use std::collections::HashMap;
use std::io;

use candid::types::Label;
use candid::types::value::VariantValue;
use candid::{IDLArgs, IDLValue, TypeEnv};
use candid_parser::{assist, parse_idl_args, utils::CandidSource};
use icp::manifest::ArgsFormat;
use icp::prelude::*;
use serde::Deserialize;
use snafu::{ResultExt, Snafu};

pub(crate) const CUSTOMIZE_FILE: &str = "icp_customize.yaml";

#[derive(Debug, Deserialize)]
pub(crate) struct CustomizeManifest {
    pub(crate) options: Vec<CustomizeOption>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CustomizeOption {
    pub(crate) canister: String,
    pub(crate) field_path: String,
    pub(crate) candid_type: String,
    pub(crate) description: String,
}

pub(crate) struct FieldPath {
    pub(crate) arg_index: usize,
    pub(crate) fields: Vec<String>,
}

#[derive(Debug, Snafu)]
pub(crate) enum LoadCustomizeManifestError {
    #[snafu(display("failed to read {path}: {source}"))]
    Read { source: io::Error, path: PathBuf },
    #[snafu(display("failed to parse {path}: {source}"))]
    Parse {
        source: serde_yaml::Error,
        path: PathBuf,
    },
}

#[derive(Debug, Snafu)]
pub(crate) enum ParseFieldPathError {
    #[snafu(display("field path is empty"))]
    Empty,
    #[snafu(display("expected integer arg index at start of {path_str:?}, got {segment:?}"))]
    InvalidIndex { segment: String, path_str: String },
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to parse Candid type {type_str:?}: {source}"))]
pub(crate) struct ParseCandidTypeError {
    source: candid_parser::Error,
    type_str: String,
}

#[derive(Debug, Snafu)]
pub(crate) enum SubstituteError {
    #[snafu(display("arg index {index} out of bounds (init args has {len} args) in {path}"))]
    ArgIndexOutOfBounds {
        index: usize,
        len: usize,
        path: PathBuf,
    },
    #[snafu(display("field {field:?} not found in record in {path}"))]
    FieldNotFound { field: String, path: PathBuf },
    #[snafu(display("cannot traverse {kind} to reach field {field:?} in {path}"))]
    NotTraversable {
        kind: &'static str,
        field: String,
        path: PathBuf,
    },
}

#[derive(Debug, Snafu)]
pub(crate) enum PromptCustomizationsError {
    #[snafu(display("invalid field_path for canister {canister:?}: {source}"))]
    FieldPath {
        source: ParseFieldPathError,
        canister: String,
    },
    #[snafu(display("invalid candid_type for canister {canister:?} at {field_path:?}: {source}"))]
    CandidType {
        source: ParseCandidTypeError,
        canister: String,
        field_path: String,
    },
    #[snafu(display("failed to parse init_args for canister {canister:?}: {source}"))]
    ParseInitArgs {
        source: candid_parser::Error,
        canister: String,
    },
    #[snafu(display(
        "init args for canister {canister:?} use a non-Candid format \
         and cannot be field-customized"
    ))]
    UnsupportedInitArgsFormat { canister: String },
    #[snafu(display("interactive prompt failed: {source}"))]
    Prompt { source: io::Error },
    #[snafu(display("{source}"))]
    Substitute { source: SubstituteError },
}

pub(crate) async fn load_customize_manifest(
    project_dir: &Path,
) -> Result<Option<CustomizeManifest>, LoadCustomizeManifestError> {
    let path = project_dir.join(CUSTOMIZE_FILE);
    match tokio::fs::read(path.as_std_path()).await {
        Ok(bytes) => {
            let m =
                serde_yaml::from_slice::<CustomizeManifest>(&bytes).context(ParseSnafu { path })?;
            Ok(Some(m))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(LoadCustomizeManifestError::Read { source, path }),
    }
}

fn parse_field_path(s: &str) -> Result<FieldPath, ParseFieldPathError> {
    if s.is_empty() {
        return Err(ParseFieldPathError::Empty);
    }
    let mut iter = s.splitn(usize::MAX, '.');
    let first = iter.next().expect("splitn always yields at least one part");
    let arg_index = first
        .parse::<usize>()
        .map_err(|_| ParseFieldPathError::InvalidIndex {
            segment: first.to_string(),
            path_str: s.to_string(),
        })?;
    let fields = iter.map(str::to_string).collect();
    Ok(FieldPath { arg_index, fields })
}

fn parse_candid_type_string(
    type_str: &str,
) -> Result<(TypeEnv, candid::types::Type), ParseCandidTypeError> {
    let source = format!("type T = {}; service : {{}}", type_str);
    let (env, _) = CandidSource::Text(&source)
        .load()
        .context(ParseCandidTypeSnafu {
            type_str: type_str.to_string(),
        })?;
    let ty = env
        .find_type("T")
        .expect("T was just defined in the synthetic source")
        .clone();
    Ok((env, ty))
}

fn idl_value_kind(v: &IDLValue) -> &'static str {
    match v {
        IDLValue::Bool(_) => "bool",
        IDLValue::Null => "null",
        IDLValue::Text(_) => "text",
        IDLValue::Number(_) => "number",
        IDLValue::Float64(_) => "float64",
        IDLValue::Float32(_) => "float32",
        IDLValue::Opt(_) => "opt",
        IDLValue::Vec(_) => "vec",
        IDLValue::Record(_) => "record",
        IDLValue::Variant(_) => "variant",
        IDLValue::Principal(_) => "principal",
        IDLValue::Service(_) => "service",
        IDLValue::Func(_, _) => "func",
        IDLValue::None => "none",
        IDLValue::Int(_) => "int",
        IDLValue::Nat(_) => "nat",
        IDLValue::Int8(_) | IDLValue::Int16(_) | IDLValue::Int32(_) | IDLValue::Int64(_) => "int_N",
        IDLValue::Nat8(_) | IDLValue::Nat16(_) | IDLValue::Nat32(_) | IDLValue::Nat64(_) => "nat_N",
        IDLValue::Reserved => "reserved",
        IDLValue::Blob(_) => "blob",
    }
}

fn substitute_value(
    value: &mut IDLValue,
    fields: &[String],
    replacement: IDLValue,
    path: &Path,
) -> Result<(), SubstituteError> {
    if fields.is_empty() {
        *value = replacement;
        return Ok(());
    }
    match value {
        IDLValue::Variant(VariantValue(inner_field, _)) => {
            // Pass through the variant without consuming a path segment.
            // The variant selection is already made in the existing init args.
            substitute_value(&mut inner_field.val, fields, replacement, path)
        }
        IDLValue::Record(record_fields) => {
            let field_name = &fields[0];
            let target_id = Label::Named(field_name.clone()).get_id();
            match record_fields
                .iter_mut()
                .find(|f| f.id.get_id() == target_id)
            {
                Some(f) => substitute_value(&mut f.val, &fields[1..], replacement, path),
                None => Err(SubstituteError::FieldNotFound {
                    field: field_name.clone(),
                    path: path.to_path_buf(),
                }),
            }
        }
        other => Err(SubstituteError::NotTraversable {
            kind: idl_value_kind(other),
            field: fields[0].clone(),
            path: path.to_path_buf(),
        }),
    }
}

pub(crate) fn substitute_field(
    args: &mut IDLArgs,
    path: &FieldPath,
    replacement: IDLValue,
    customize_path: &Path,
) -> Result<(), SubstituteError> {
    if path.arg_index >= args.args.len() {
        return Err(SubstituteError::ArgIndexOutOfBounds {
            index: path.arg_index,
            len: args.args.len(),
            path: customize_path.to_path_buf(),
        });
    }
    substitute_value(
        &mut args.args[path.arg_index],
        &path.fields,
        replacement,
        customize_path,
    )
}

pub(crate) fn prompt_customizations(
    manifest: &CustomizeManifest,
    cnames: &[String],
    init_args: &HashMap<String, Option<icp::InitArgs>>,
    skip: bool,
    customize_path: &Path,
) -> Result<HashMap<String, IDLArgs>, PromptCustomizationsError> {
    if skip {
        return Ok(HashMap::new());
    }

    let cname_set: std::collections::HashSet<&str> = cnames.iter().map(String::as_str).collect();

    // Group by canister preserving declaration order, filtered to deployed canisters.
    let mut by_canister: Vec<(&str, Vec<&CustomizeOption>)> = Vec::new();
    for opt in &manifest.options {
        if !cname_set.contains(opt.canister.as_str()) {
            continue;
        }
        match by_canister
            .iter_mut()
            .find(|(name, _)| *name == opt.canister.as_str())
        {
            Some((_, opts)) => opts.push(opt),
            None => by_canister.push((opt.canister.as_str(), vec![opt])),
        }
    }

    let mut result = HashMap::new();

    for (canister_name, options) in &by_canister {
        let mut working_args = match init_args
            .get(*canister_name)
            .and_then(Option::as_ref)
            .cloned()
        {
            None => IDLArgs { args: vec![] },
            Some(icp::InitArgs::Text {
                content,
                format: ArgsFormat::Candid,
            }) => parse_idl_args(content.trim()).context(ParseInitArgsSnafu {
                canister: *canister_name,
            })?,
            Some(icp::InitArgs::Text { .. } | icp::InitArgs::Binary(_)) => {
                return Err(PromptCustomizationsError::UnsupportedInitArgsFormat {
                    canister: canister_name.to_string(),
                });
            }
        };

        for opt in options {
            let field_path = parse_field_path(&opt.field_path).context(FieldPathSnafu {
                canister: canister_name.to_string(),
            })?;

            let (env, ty) =
                parse_candid_type_string(&opt.candid_type).context(CandidTypeSnafu {
                    canister: canister_name.to_string(),
                    field_path: opt.field_path.clone(),
                })?;

            eprintln!("[{}] {}", canister_name, opt.description);

            let context = assist::Context::new(env);
            let prompted = assist::input_args(&context, &[ty]).map_err(|e| {
                PromptCustomizationsError::Prompt {
                    source: io::Error::new(io::ErrorKind::Other, e.to_string()),
                }
            })?;

            let value = prompted
                .args
                .into_iter()
                .next()
                .expect("input_args returns one value per type element");

            substitute_field(&mut working_args, &field_path, value, customize_path)
                .context(SubstituteSnafu)?;
        }

        result.insert(canister_name.to_string(), working_args);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino_tempfile::Utf8TempDir;
    use candid::types::value::IDLField;

    fn nat64_record_args(supply: u64) -> IDLArgs {
        IDLArgs {
            args: vec![IDLValue::Record(vec![IDLField {
                id: Label::Named("supply".to_string()),
                val: IDLValue::Nat64(supply),
            }])],
        }
    }

    #[test]
    fn parse_field_path_index_only() {
        let fp = parse_field_path("0").unwrap();
        assert_eq!(fp.arg_index, 0);
        assert!(fp.fields.is_empty());
    }

    #[test]
    fn parse_field_path_with_fields() {
        let fp = parse_field_path("0.supply").unwrap();
        assert_eq!(fp.arg_index, 0);
        assert_eq!(fp.fields, vec!["supply"]);
    }

    #[test]
    fn parse_field_path_nested() {
        let fp = parse_field_path("1.a.b.c").unwrap();
        assert_eq!(fp.arg_index, 1);
        assert_eq!(fp.fields, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_field_path_empty_err() {
        assert!(matches!(
            parse_field_path(""),
            Err(ParseFieldPathError::Empty)
        ));
    }

    #[test]
    fn parse_field_path_non_integer_index_err() {
        assert!(matches!(
            parse_field_path("foo.bar"),
            Err(ParseFieldPathError::InvalidIndex { .. })
        ));
    }

    #[test]
    fn substitute_simple_field() {
        let mut args = nat64_record_args(0);
        let path = parse_field_path("0.supply").unwrap();
        substitute_field(
            &mut args,
            &path,
            IDLValue::Nat64(42),
            Path::new("test.yaml"),
        )
        .unwrap();
        if let IDLValue::Record(fields) = &args.args[0] {
            assert!(matches!(fields[0].val, IDLValue::Nat64(42)));
        } else {
            panic!("expected record");
        }
    }

    #[test]
    fn substitute_out_of_bounds_err() {
        let mut args = IDLArgs { args: vec![] };
        let path = parse_field_path("0").unwrap();
        let err =
            substitute_field(&mut args, &path, IDLValue::Null, Path::new("test.yaml")).unwrap_err();
        assert!(matches!(
            err,
            SubstituteError::ArgIndexOutOfBounds {
                index: 0,
                len: 0,
                ..
            }
        ));
    }

    #[test]
    fn substitute_field_not_found_err() {
        let mut args = nat64_record_args(0);
        let path = parse_field_path("0.missing").unwrap();
        let err = substitute_field(&mut args, &path, IDLValue::Nat64(1), Path::new("test.yaml"))
            .unwrap_err();
        assert!(matches!(err, SubstituteError::FieldNotFound { .. }));
    }

    #[test]
    fn substitute_passes_through_variant() {
        // Structure: record { status = variant { active = record { value = 0 : nat64 } } }
        // The variant is transparent in the path: "0.status.value" navigates through the variant.
        let payload_field = IDLField {
            id: Label::Named("value".to_string()),
            val: IDLValue::Nat64(0),
        };
        let variant_inner = IDLField {
            id: Label::Named("active".to_string()),
            val: IDLValue::Record(vec![payload_field]),
        };
        let status_field = IDLField {
            id: Label::Named("status".to_string()),
            val: IDLValue::Variant(VariantValue(Box::new(variant_inner), 0)),
        };
        let mut args = IDLArgs {
            args: vec![IDLValue::Record(vec![status_field])],
        };
        let path = parse_field_path("0.status.value").unwrap();
        substitute_field(
            &mut args,
            &path,
            IDLValue::Nat64(99),
            Path::new("test.yaml"),
        )
        .unwrap();

        if let IDLValue::Record(fields) = &args.args[0] {
            if let IDLValue::Variant(VariantValue(inner, _)) = &fields[0].val {
                if let IDLValue::Record(payload_fields) = &inner.val {
                    assert!(matches!(payload_fields[0].val, IDLValue::Nat64(99)));
                    return;
                }
            }
        }
        panic!("unexpected args structure");
    }

    #[test]
    fn substitute_not_traversable_err() {
        let mut args = IDLArgs {
            args: vec![IDLValue::Nat64(0)],
        };
        let path = parse_field_path("0.field").unwrap();
        let err = substitute_field(&mut args, &path, IDLValue::Nat64(1), Path::new("test.yaml"))
            .unwrap_err();
        assert!(matches!(err, SubstituteError::NotTraversable { .. }));
    }

    #[test]
    fn parse_candid_type_nat64() {
        let (_, ty) = parse_candid_type_string("nat64").unwrap();
        assert!(matches!(ty.as_ref(), candid::types::TypeInner::Nat64));
    }

    #[test]
    fn parse_candid_type_principal() {
        let (_, ty) = parse_candid_type_string("principal").unwrap();
        assert!(matches!(ty.as_ref(), candid::types::TypeInner::Principal));
    }

    #[test]
    fn parse_candid_type_invalid_err() {
        assert!(parse_candid_type_string("@@@invalid").is_err());
    }

    #[test]
    fn prompt_skip_returns_empty() {
        let manifest = CustomizeManifest {
            options: vec![CustomizeOption {
                canister: "c".to_string(),
                field_path: "0.x".to_string(),
                candid_type: "nat64".to_string(),
                description: "desc".to_string(),
            }],
        };
        let result = prompt_customizations(
            &manifest,
            &["c".to_string()],
            &HashMap::new(),
            true,
            Path::new("icp_customize.yaml"),
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn load_missing_file_returns_none() {
        let tmp = Utf8TempDir::new().unwrap();
        let result = load_customize_manifest(tmp.path()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn load_valid_file() {
        let tmp = Utf8TempDir::new().unwrap();
        let content = r#"
options:
  - canister: my-canister
    field_path: "0.supply"
    candid_type: "nat64"
    description: "Initial supply"
"#;
        std::fs::write(tmp.path().join(CUSTOMIZE_FILE), content).unwrap();
        let manifest = load_customize_manifest(tmp.path()).await.unwrap().unwrap();
        assert_eq!(manifest.options.len(), 1);
        assert_eq!(manifest.options[0].canister, "my-canister");
    }

    #[tokio::test]
    async fn load_malformed_file_err() {
        let tmp = Utf8TempDir::new().unwrap();
        std::fs::write(tmp.path().join(CUSTOMIZE_FILE), "options: }{bad yaml").unwrap();
        let err = load_customize_manifest(tmp.path()).await.unwrap_err();
        assert!(matches!(err, LoadCustomizeManifestError::Parse { .. }));
    }
}
