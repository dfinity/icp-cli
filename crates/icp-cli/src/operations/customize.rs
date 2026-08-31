use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::io;

use camino::Utf8Component;
use candid::types::Label;
use candid::types::value::VariantValue;
use candid::{IDLArgs, IDLValue, TypeEnv};
use candid_parser::{assist, parse_idl_args, utils::CandidSource};
use icp::fs::yaml;
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
    pub(crate) canister: CanisterRef,
    pub(crate) field_path: String,
    pub(crate) candid_type: String,
    pub(crate) description: String,
}

/// The canister an option customizes: its local name, optionally preceded by the
/// workspace-relative path of the project that declares it — `backend` for a
/// canister of the root project, `services/open-crm:backend` for one belonging to
/// a vendored dependency. This is the store key the rest of the CLI addresses that
/// canister by, so [`Self::store_key`] round-trips into `icp deploy`'s naming.
///
/// A workspace has one customizations file, at its root; the prefix is how that
/// file reaches into a member project.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct CanisterRef {
    /// Owning project's directory relative to the workspace root, forward-slash
    /// separated with no `.` segments. Empty for the root project itself.
    project: String,
    local: String,
}

impl CanisterRef {
    /// Split a reference at its last `:`, the same way consolidation splits a
    /// store key — a canister's local name never contains one, while a project
    /// path can (a Windows drive prefix, which normalization then rejects).
    fn parse(reference: &str) -> Result<Self, ParseCanisterRefError> {
        if reference.is_empty() {
            return Err(ParseCanisterRefError::EmptyReference);
        }
        let (project, local) = match reference.rsplit_once(':') {
            Some((project, local)) => (normalize_project_path(project, reference)?, local),
            None => (String::new(), reference),
        };
        if local.is_empty() {
            return EmptyCanisterNameSnafu { reference }.fail();
        }
        Ok(Self {
            project,
            local: local.to_owned(),
        })
    }

    /// The consolidated name of this canister in the workspace: a root project's
    /// canisters keep their bare local names, a member's are prefixed.
    pub(crate) fn store_key(&self) -> String {
        match self.project.is_empty() {
            true => self.local.clone(),
            false => format!("{}:{}", self.project, self.local),
        }
    }
}

impl fmt::Display for CanisterRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.store_key())
    }
}

impl TryFrom<String> for CanisterRef {
    type Error = ParseCanisterRefError;

    fn try_from(reference: String) -> Result<Self, Self::Error> {
        Self::parse(&reference)
    }
}

/// Restate a hand-written project path in the spelling store-key prefixes use, so
/// `./services/open-crm` and `services/open-crm/` address the same member as
/// `services/open-crm`. `..` survives: a dependency can be declared through a
/// path that leaves the workspace root.
fn normalize_project_path(project: &str, reference: &str) -> Result<String, ParseCanisterRefError> {
    let slashed = project.replace('\\', "/");
    let mut segments: Vec<&str> = Vec::new();
    for component in Path::new(&slashed).components() {
        match component {
            Utf8Component::CurDir => {}
            Utf8Component::ParentDir => segments.push(".."),
            Utf8Component::Normal(segment) => segments.push(segment),
            Utf8Component::Prefix(_) | Utf8Component::RootDir => {
                return AbsoluteProjectPathSnafu { project, reference }.fail();
            }
        }
    }
    Ok(segments.join("/"))
}

#[derive(Debug)]
pub(crate) struct FieldPath {
    pub(crate) arg_index: usize,
    pub(crate) fields: Vec<String>,
}

pub(crate) type LoadCustomizeManifestError = yaml::Error;

#[derive(Debug, Snafu)]
pub(crate) enum ParseCanisterRefError {
    #[snafu(display("canister reference is empty"))]
    EmptyReference,

    #[snafu(display(
        "canister reference {reference:?} names no canister after its ':' — \
         write the project path first, like \"services/open-crm:backend\""
    ))]
    EmptyCanisterName { reference: String },

    #[snafu(display(
        "project path {project:?} in canister reference {reference:?} is absolute; \
         it must be relative to the workspace root, like \"services/open-crm:backend\""
    ))]
    AbsoluteProjectPath { project: String, reference: String },
}

#[derive(Debug, Snafu)]
#[snafu(display(
    "canister {reference:?} in {path} is not part of this workspace. \
     Its canisters are: {known}"
))]
pub(crate) struct UnknownCanisterError {
    reference: String,
    known: String,
    path: PathBuf,
}

#[derive(Debug, Snafu)]
pub(crate) enum ParseFieldPathError {
    #[snafu(display("field path is empty"))]
    Empty,
    #[snafu(display(
        "field path {path_str:?} must start with an arg index — \
         try \".{path_str}\" (shorthand for arg 0) or \"<n>.{path_str}\""
    ))]
    InvalidIndex { path_str: String },
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to parse Candid type {type_str:?}"))]
pub(crate) struct ParseCandidTypeError {
    #[snafu(source(from(candid_parser::Error, Box::new)))]
    source: Box<candid_parser::Error>,
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
    #[snafu(display("invalid field_path for canister {canister:?} in {path}"))]
    FieldPath {
        source: ParseFieldPathError,
        canister: String,
        path: PathBuf,
    },
    #[snafu(display("invalid candid_type for canister {canister:?} at {field_path:?} in {path}"))]
    CandidType {
        source: ParseCandidTypeError,
        canister: String,
        field_path: String,
        path: PathBuf,
    },
    #[snafu(display("failed to parse init_args for canister {canister:?} in {path}"))]
    ParseInitArgs {
        #[snafu(source(from(candid_parser::Error, Box::new)))]
        source: Box<candid_parser::Error>,
        canister: String,
        path: PathBuf,
    },
    #[snafu(display(
        "init args for canister {canister:?} use a non-Candid format \
         and cannot be field-customized (referenced from {path})"
    ))]
    UnsupportedInitArgsFormat { canister: String, path: PathBuf },
    #[snafu(display(
        "interactive prompt failed for canister {canister:?} at {field_path:?} (from {path})"
    ))]
    Prompt {
        source: anyhow::Error,
        canister: String,
        field_path: String,
        path: PathBuf,
    },
    #[snafu(transparent)]
    Substitute { source: SubstituteError },
}

pub(crate) fn load_customize_manifest(
    project_dir: &Path,
) -> Result<Option<CustomizeManifest>, LoadCustomizeManifestError> {
    let path = project_dir.join(CUSTOMIZE_FILE);
    match yaml::load::<CustomizeManifest>(&path) {
        Ok(m) => Ok(Some(m)),
        Err(yaml::Error::Io { source }) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Check every option against the canisters the workspace actually declares.
///
/// Without this, a misspelled project path is indistinguishable from a canister
/// that simply isn't part of the current deploy, and
/// [`prompt_customizations`] would skip it with a warning — leaving the canister
/// to be installed with unintended init args.
pub(crate) fn validate_canister_refs(
    manifest: &CustomizeManifest,
    workspace_canisters: &[&str],
    customize_path: &Path,
) -> Result<(), UnknownCanisterError> {
    let known: BTreeSet<&str> = workspace_canisters.iter().copied().collect();
    for opt in &manifest.options {
        let store_key = opt.canister.store_key();
        if !known.contains(store_key.as_str()) {
            return UnknownCanisterSnafu {
                reference: store_key,
                known: known.iter().copied().collect::<Vec<_>>().join(", "),
                path: customize_path,
            }
            .fail();
        }
    }
    Ok(())
}

/// Warn about a customizations file belonging to a member project, whose options
/// are not read.
///
/// A workspace collects its prompts once, in the root's file, where a
/// `<project path>:` prefix reaches a member's canisters. A member's own file is
/// still legitimate — it is what that project uses when deployed on its own — so
/// finding one is not an error, but its options are silently inert here, which
/// would install the canister with unintended init args.
///
/// Members are read off the store keys of `workspace_canisters`, whose prefix is
/// the owning project's workspace-relative directory.
pub(crate) fn warn_unread_member_customize_files(root_dir: &Path, workspace_canisters: &[&str]) {
    let members: BTreeSet<&str> = workspace_canisters
        .iter()
        .filter_map(|name| name.rsplit_once(':'))
        .map(|(project, _)| project)
        .filter(|project| !project.is_empty())
        .collect();

    for member in members {
        let path = root_dir.join(member).join(CUSTOMIZE_FILE);
        if path.is_file() {
            tracing::warn!(
                "Ignoring '{path}': a workspace declares its customizations once, in \
                 '{CUSTOMIZE_FILE}' at its root. To prompt for these, copy the options there \
                 and prefix each `canister` with '{member}:'."
            );
        }
    }
}

fn parse_field_path(s: &str) -> Result<FieldPath, ParseFieldPathError> {
    if s.is_empty() {
        return Err(ParseFieldPathError::Empty);
    }
    if let Some(rest) = s.strip_prefix('.') {
        let fields = if rest.is_empty() {
            vec![]
        } else {
            rest.split('.').map(str::to_string).collect()
        };
        return Ok(FieldPath {
            arg_index: 0,
            fields,
        });
    }
    let mut iter = s.split('.');
    let first = iter.next().expect("split always yields at least one part");
    let arg_index = first
        .parse::<usize>()
        .map_err(|_| ParseFieldPathError::InvalidIndex {
            path_str: s.to_string(),
        })?;
    let fields = iter.map(str::to_string).collect();
    Ok(FieldPath { arg_index, fields })
}

fn parse_contextfree_candid_type_string(
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

    let cname_set: HashSet<&str> = cnames.iter().map(String::as_str).collect();

    // Group by canister preserving declaration order, filtered to the canisters
    // this deploy targets. A reference that resolves to no canister at all is
    // rejected by `validate_canister_refs`, so what is skipped here is only ever
    // a canister outside the current scope — worth reporting, since a
    // member-scoped deploy silently drops the rest of the workspace's options.
    let mut by_canister: Vec<(String, Vec<&CustomizeOption>)> = Vec::new();
    let mut skipped: BTreeSet<String> = BTreeSet::new();
    for opt in &manifest.options {
        let store_key = opt.canister.store_key();
        if !cname_set.contains(store_key.as_str()) {
            skipped.insert(store_key);
            continue;
        }
        match by_canister.iter_mut().find(|(name, _)| *name == store_key) {
            Some((_, opts)) => opts.push(opt),
            None => by_canister.push((store_key, vec![opt])),
        }
    }
    if !skipped.is_empty() {
        let names = skipped
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        tracing::warn!(
            "Customize options skipped because their canister is not being deployed: {names}"
        );
    }

    let mut result = HashMap::new();

    for (canister_name, options) in &by_canister {
        let canister_name = canister_name.as_str();
        let mut working_args = match init_args
            .get(canister_name)
            .and_then(Option::as_ref)
            .cloned()
        {
            None => IDLArgs { args: vec![] },
            Some(icp::InitArgs::Text {
                content,
                format: ArgsFormat::Candid,
            }) => parse_idl_args(content.trim()).context(ParseInitArgsSnafu {
                canister: canister_name,
                path: customize_path,
            })?,
            Some(icp::InitArgs::Text { .. } | icp::InitArgs::Binary(_)) => {
                return UnsupportedInitArgsFormatSnafu {
                    canister: canister_name,
                    path: customize_path,
                }
                .fail();
            }
        };

        for opt in options {
            let field_path = parse_field_path(&opt.field_path).context(FieldPathSnafu {
                canister: canister_name,
                path: customize_path,
            })?;

            let (env, ty) = parse_contextfree_candid_type_string(&opt.candid_type).context(
                CandidTypeSnafu {
                    canister: canister_name,
                    field_path: opt.field_path.as_str(),
                    path: customize_path,
                },
            )?;

            eprintln!("[{}] {}", canister_name, opt.description);

            let context = assist::Context::new(env);
            let prompted = assist::input_args(&context, &[ty]).context(PromptSnafu {
                canister: canister_name,
                field_path: opt.field_path.as_str(),
                path: customize_path,
            })?;

            let value = prompted
                .args
                .into_iter()
                .next()
                .expect("input_args returns one value per type element");

            substitute_field(&mut working_args, &field_path, value, customize_path)?;
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

    fn cref(reference: &str) -> CanisterRef {
        CanisterRef::parse(reference).expect("reference should parse")
    }

    fn option_for(reference: &str) -> CustomizeOption {
        CustomizeOption {
            canister: cref(reference),
            field_path: ".x".to_string(),
            candid_type: "nat64".to_string(),
            description: "desc".to_string(),
        }
    }

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
    fn parse_field_path_dot_shorthand() {
        let fp = parse_field_path(".supply").unwrap();
        assert_eq!(fp.arg_index, 0);
        assert_eq!(fp.fields, vec!["supply"]);
    }

    #[test]
    fn parse_field_path_dot_shorthand_nested() {
        let fp = parse_field_path(".a.b.c").unwrap();
        assert_eq!(fp.arg_index, 0);
        assert_eq!(fp.fields, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_field_path_bare_dot() {
        let fp = parse_field_path(".").unwrap();
        assert_eq!(fp.arg_index, 0);
        assert!(fp.fields.is_empty());
    }

    #[test]
    fn parse_field_path_bare_field_error_suggests_shorthand() {
        let err = parse_field_path("field1").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("\".field1\""), "message was: {msg}");
        assert!(msg.contains("shorthand for arg 0"), "message was: {msg}");
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

        if let IDLValue::Record(fields) = &args.args[0]
            && let IDLValue::Variant(VariantValue(inner, _)) = &fields[0].val
            && let IDLValue::Record(payload_fields) = &inner.val
        {
            assert!(matches!(payload_fields[0].val, IDLValue::Nat64(99)));
            return;
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
        let (_, ty) = parse_contextfree_candid_type_string("nat64").unwrap();
        assert!(matches!(ty.as_ref(), candid::types::TypeInner::Nat64));
    }

    #[test]
    fn parse_candid_type_principal() {
        let (_, ty) = parse_contextfree_candid_type_string("principal").unwrap();
        assert!(matches!(ty.as_ref(), candid::types::TypeInner::Principal));
    }

    #[test]
    fn parse_candid_type_invalid_err() {
        assert!(parse_contextfree_candid_type_string("@@@invalid").is_err());
    }

    #[test]
    fn prompt_skip_returns_empty() {
        let manifest = CustomizeManifest {
            options: vec![option_for("c")],
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

    #[test]
    fn load_missing_file_returns_none() {
        let tmp = Utf8TempDir::new().unwrap();
        let result = load_customize_manifest(tmp.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn load_valid_file() {
        let tmp = Utf8TempDir::new().unwrap();
        let content = r#"
options:
  - canister: my-canister
    field_path: "0.supply"
    candid_type: "nat64"
    description: "Initial supply"
"#;
        std::fs::write(tmp.path().join(CUSTOMIZE_FILE), content).unwrap();
        let manifest = load_customize_manifest(tmp.path()).unwrap().unwrap();
        assert_eq!(manifest.options.len(), 1);
        assert_eq!(manifest.options[0].canister, cref("my-canister"));
    }

    #[test]
    fn load_malformed_file_err() {
        let tmp = Utf8TempDir::new().unwrap();
        std::fs::write(tmp.path().join(CUSTOMIZE_FILE), "options: }{bad yaml").unwrap();
        let err = load_customize_manifest(tmp.path()).unwrap_err();
        assert!(matches!(err, LoadCustomizeManifestError::Parse { .. }));
    }

    #[test]
    fn prompt_rejects_binary_init_args() {
        // Surfaces the format check before any interactive prompt by giving the canister
        // non-Candid init args.
        let manifest = CustomizeManifest {
            options: vec![option_for("c")],
        };
        let init_args = HashMap::from([("c".to_string(), Some(icp::InitArgs::Binary(vec![0u8])))]);
        let err = prompt_customizations(
            &manifest,
            &["c".to_string()],
            &init_args,
            false,
            Path::new("icp_customize.yaml"),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            PromptCustomizationsError::UnsupportedInitArgsFormat { .. }
        ));
        let msg = err.to_string();
        assert!(msg.contains("non-Candid format"), "got: {msg}");
        assert!(msg.contains("icp_customize.yaml"), "got: {msg}");
    }

    #[test]
    fn prompt_returns_empty_when_no_options_match_deployment() {
        // Manifest targets canister "a", deployment is for "b" — every option is filtered
        // out, no prompts fire, the result is empty.
        let manifest = CustomizeManifest {
            options: vec![option_for("a")],
        };
        let result = prompt_customizations(
            &manifest,
            &["b".to_string()],
            &HashMap::new(),
            false,
            Path::new("icp_customize.yaml"),
        )
        .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn parse_ref_without_project_is_root() {
        let r = cref("backend");
        assert_eq!(r.project, "");
        assert_eq!(r.local, "backend");
        assert_eq!(r.store_key(), "backend");
    }

    #[test]
    fn parse_ref_with_project_prefix() {
        let r = cref("services/open-crm:backend");
        assert_eq!(r.project, "services/open-crm");
        assert_eq!(r.local, "backend");
        assert_eq!(r.store_key(), "services/open-crm:backend");
    }

    #[test]
    fn parse_ref_normalizes_project_spelling() {
        // The store-key prefix is a clean forward-slash path relative to the
        // workspace root; these spellings all name the same member.
        for reference in [
            "./services/open-crm:backend",
            "services/open-crm/:backend",
            "services/./open-crm:backend",
            r"services\open-crm:backend",
        ] {
            assert_eq!(
                cref(reference).store_key(),
                "services/open-crm:backend",
                "reference was: {reference}"
            );
        }
    }

    #[test]
    fn parse_ref_keeps_parent_segments() {
        // A dependency can be declared through a path that leaves the workspace
        // root, and its store-key prefix keeps the `..`.
        assert_eq!(cref("../shared:backend").store_key(), "../shared:backend");
    }

    #[test]
    fn parse_ref_bare_project_is_root() {
        // A prefix that normalizes away addresses the root project, whose
        // canisters are keyed by their bare names.
        assert_eq!(cref(":backend").store_key(), "backend");
        assert_eq!(cref(".:backend").store_key(), "backend");
    }

    #[test]
    fn parse_ref_empty_err() {
        assert!(matches!(
            CanisterRef::parse(""),
            Err(ParseCanisterRefError::EmptyReference)
        ));
    }

    #[test]
    fn parse_ref_missing_canister_name_err() {
        let err = CanisterRef::parse("services/open-crm:").unwrap_err();
        assert!(matches!(
            err,
            ParseCanisterRefError::EmptyCanisterName { .. }
        ));
    }

    #[test]
    fn parse_ref_absolute_project_err() {
        let err = CanisterRef::parse("/srv/open-crm:backend").unwrap_err();
        assert!(matches!(
            err,
            ParseCanisterRefError::AbsoluteProjectPath { .. }
        ));
        let msg = err.to_string();
        assert!(msg.contains("relative to the workspace root"), "got: {msg}");
    }

    #[test]
    fn load_file_with_project_prefix() {
        let tmp = Utf8TempDir::new().unwrap();
        let content = r#"
options:
  - canister: services/open-crm:backend
    field_path: ".supply"
    candid_type: "nat64"
    description: "Initial supply"
"#;
        std::fs::write(tmp.path().join(CUSTOMIZE_FILE), content).unwrap();
        let manifest = load_customize_manifest(tmp.path()).unwrap().unwrap();
        assert_eq!(
            manifest.options[0].canister,
            cref("services/open-crm:backend")
        );
    }

    #[test]
    fn load_file_with_absolute_project_err() {
        // The reference is rejected while deserializing, so the failure names the
        // file rather than surfacing later as a missing canister.
        let tmp = Utf8TempDir::new().unwrap();
        let content = r#"
options:
  - canister: /srv/open-crm:backend
    field_path: ".supply"
    candid_type: "nat64"
    description: "Initial supply"
"#;
        std::fs::write(tmp.path().join(CUSTOMIZE_FILE), content).unwrap();
        let err = load_customize_manifest(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains(CUSTOMIZE_FILE), "got: {msg}");
    }

    #[test]
    fn validate_accepts_member_canister() {
        let manifest = CustomizeManifest {
            options: vec![option_for("services/open-crm:backend")],
        };
        validate_canister_refs(
            &manifest,
            &["frontend", "services/open-crm:backend"],
            Path::new(CUSTOMIZE_FILE),
        )
        .unwrap();
    }

    #[test]
    fn validate_rejects_misspelled_project() {
        // The canister exists, but under a different member — the kind of mistake
        // the scope filter in `prompt_customizations` would otherwise swallow.
        let manifest = CustomizeManifest {
            options: vec![option_for("services/open_crm:backend")],
        };
        let err = validate_canister_refs(
            &manifest,
            &["frontend", "services/open-crm:backend"],
            Path::new(CUSTOMIZE_FILE),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("services/open_crm:backend"), "got: {msg}");
        assert!(msg.contains("services/open-crm:backend"), "got: {msg}");
    }

    #[test]
    fn validate_rejects_member_canister_named_without_prefix() {
        // A member's canister is only reachable through its project path, so the
        // bare local name is not a canister of the workspace.
        let manifest = CustomizeManifest {
            options: vec![option_for("backend")],
        };
        assert!(
            validate_canister_refs(
                &manifest,
                &["services/open-crm:backend"],
                Path::new(CUSTOMIZE_FILE),
            )
            .is_err()
        );
    }
}
