use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::io;
use std::ops::Deref;

use camino::Utf8Component;
use candid::types::Label;
use candid::types::value::VariantValue;
use candid::types::{TypeEnv, TypeInner};
use candid::{IDLArgs, IDLValue};
use candid_parser::{assist, parse_idl_args, utils::CandidSource};
use icp::canister::Settings;
use icp::fs::yaml;
use icp::manifest::ArgsFormat;
use icp::prelude::*;
use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use snafu::{ResultExt, Snafu};

pub(crate) const CUSTOMIZE_FILE: &str = "icp_customize.yaml";

#[derive(Debug, Deserialize)]
pub(crate) struct CustomizeManifest {
    pub(crate) options: Vec<CustomizeOption>,
}

/// One user-configurable setting: what to ask about, which canisters it belongs
/// to, and where the answer goes.
#[derive(Debug, Deserialize)]
#[serde(try_from = "CustomizeOptionRepr")]
pub(crate) struct CustomizeOption {
    pub(crate) canister: CanisterRefs,
    pub(crate) description: String,
    pub(crate) target: CustomizeTarget,
}

/// What an option's answer configures. Each option addresses one or the other.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CustomizeTarget {
    /// A field of the init args the canister is installed with, addressed by
    /// `field_path` and asked for at `candid_type`.
    InitArgField {
        field_path: String,
        candid_type: String,
    },
    /// One of the canister's environment variables. A variable's value is text,
    /// so there is no type to declare, and the variable need not be among the
    /// canister's manifest settings — the answer is laid over them either way.
    EnvVar { name: String },
}

impl CustomizeTarget {
    /// How the destination is named in messages about the option.
    fn describe(&self) -> String {
        match self {
            Self::InitArgField { field_path, .. } => format!("field path {field_path:?}"),
            Self::EnvVar { name } => format!("environment variable {name:?}"),
        }
    }
}

/// A [`CustomizeOption`] as the file spells it, with both targets' fields flat
/// and optional. Written this way rather than as an untagged enum, which rejects
/// a half-written option as "did not match any variant" — saying nothing about
/// which half is missing.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CustomizeOptionRepr {
    canister: CanisterRefs,
    description: String,
    field_path: Option<String>,
    candid_type: Option<String>,
    env: Option<String>,
}

impl TryFrom<CustomizeOptionRepr> for CustomizeOption {
    type Error = ParseCustomizeOptionError;

    fn try_from(repr: CustomizeOptionRepr) -> Result<Self, Self::Error> {
        let CustomizeOptionRepr {
            canister,
            description,
            field_path,
            candid_type,
            env,
        } = repr;
        let target = match (field_path, candid_type, env) {
            (Some(field_path), Some(candid_type), None) => CustomizeTarget::InitArgField {
                field_path,
                candid_type,
            },
            (None, None, Some(name)) if name.is_empty() => {
                return Err(ParseCustomizeOptionError::EmptyEnvVarName);
            }
            // `env: KEY=value` is the shape of a shell assignment, not of a name.
            (None, None, Some(name)) if name.contains('=') => {
                return EnvVarNameHasEqualsSnafu { name }.fail();
            }
            (None, None, Some(name)) => CustomizeTarget::EnvVar { name },
            (_, _, Some(_)) => return Err(ParseCustomizeOptionError::MixedTarget),
            (Some(_), None, None) => return Err(ParseCustomizeOptionError::MissingCandidType),
            (None, Some(_), None) => return Err(ParseCustomizeOptionError::MissingFieldPath),
            (None, None, None) => return Err(ParseCustomizeOptionError::NoTarget),
        };
        Ok(Self {
            canister,
            description,
            target,
        })
    }
}

/// The canisters one option applies to: written as a single reference, or as a
/// list to ask for the field once and give every canister in it the same answer.
/// Never empty — an option applying to nothing would prompt for nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CanisterRefs(Vec<CanisterRef>);

impl CanisterRefs {
    /// The references as one string, naming the option as a whole. Which of
    /// several canisters a failure belongs to is reported separately.
    fn joined(&self) -> String {
        self.0
            .iter()
            .map(CanisterRef::store_key)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Deref for CanisterRefs {
    type Target = [CanisterRef];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'de> Deserialize<'de> for CanisterRefs {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Written by hand rather than as an untagged enum, which reports a
        // malformed reference as "did not match any variant" and throws away
        // what `CanisterRef::parse` had to say about it.
        struct OneOrMany;

        impl<'de> Visitor<'de> for OneOrMany {
            type Value = CanisterRefs;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a canister reference, or a list of canister references")
            }

            fn visit_str<E: de::Error>(self, reference: &str) -> Result<Self::Value, E> {
                CanisterRef::parse(reference)
                    .map(|r| CanisterRefs(vec![r]))
                    .map_err(de::Error::custom)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut refs = Vec::with_capacity(seq.size_hint().unwrap_or(1));
                while let Some(reference) = seq.next_element::<CanisterRef>()? {
                    refs.push(reference);
                }
                if refs.is_empty() {
                    return Err(de::Error::custom(
                        "`canister` is an empty list; name at least one canister",
                    ));
                }
                Ok(CanisterRefs(refs))
            }
        }

        deserializer.deserialize_any(OneOrMany)
    }
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
    /// The path as the manifest spells it, for messages about where a value
    /// could not be applied.
    pub(crate) text: String,
}

pub(crate) type LoadCustomizeManifestError = yaml::Error;

#[derive(Debug, Snafu)]
pub(crate) enum ParseCustomizeOptionError {
    #[snafu(display(
        "an option customizes an init argument field or an environment variable, not both — \
         give it either `field_path` and `candid_type`, or `env`"
    ))]
    MixedTarget,

    #[snafu(display("option has a `field_path` but no `candid_type` to prompt for it at"))]
    MissingCandidType,

    #[snafu(display("option has a `candid_type` but no `field_path` to substitute it into"))]
    MissingFieldPath,

    #[snafu(display(
        "option customizes nothing — give it `field_path` and `candid_type` to set a field \
         of the init args, or `env` to set an environment variable"
    ))]
    NoTarget,

    #[snafu(display("`env` names no environment variable"))]
    EmptyEnvVarName,

    #[snafu(display(
        "environment variable name {name:?} contains '='; `env` takes the name alone, \
         and the value is what the option prompts for"
    ))]
    EnvVarNameHasEquals { name: String },
}

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
    #[snafu(display("invalid field_path for canister(s) {canisters:?} in {path}"))]
    FieldPath {
        source: ParseFieldPathError,
        canisters: String,
        path: PathBuf,
    },
    #[snafu(display(
        "invalid candid_type for canister(s) {canisters:?} at {field_path:?} in {path}"
    ))]
    CandidType {
        source: ParseCandidTypeError,
        canisters: String,
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
        "interactive prompt failed for canister(s) {canisters:?} at {target} (from {path})"
    ))]
    Prompt {
        source: anyhow::Error,
        canisters: String,
        target: String,
        path: PathBuf,
    },
    // One option can apply its answer to several canisters, so which canister
    // rejected the value is not implied by the option alone.
    #[snafu(display("cannot apply the value for {field_path:?} to canister {canister:?}"))]
    Substitute {
        source: SubstituteError,
        canister: String,
        field_path: String,
    },
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
        for reference in opt.canister.iter() {
            let store_key = reference.store_key();
            if !known.contains(store_key.as_str()) {
                return UnknownCanisterSnafu {
                    reference: store_key,
                    known: known.iter().copied().collect::<Vec<_>>().join(", "),
                    path: customize_path,
                }
                .fail();
            }
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
            text: s.to_string(),
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
    Ok(FieldPath {
        arg_index,
        fields,
        text: s.to_string(),
    })
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
        IDLValue::Opt(inner) => {
            // Likewise for an optional: the existing init args already decided
            // the value is present, so the path reaches straight into it. An
            // absent one is not an `Opt` at all and falls through as an error —
            // there is no payload to place a field in.
            substitute_value(inner, fields, replacement, path)
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

/// A canister's init args from the manifest, as the Candid values the option's
/// answers are substituted into. A canister with none starts from no args, which
/// only a whole-argument field path (`0`) can then fill.
fn manifest_init_args(
    init_args: &HashMap<String, Option<icp::InitArgs>>,
    canister: &str,
    customize_path: &Path,
) -> Result<IDLArgs, PromptCustomizationsError> {
    match init_args.get(canister).and_then(Option::as_ref).cloned() {
        None => Ok(IDLArgs { args: vec![] }),
        Some(icp::InitArgs::Text {
            content,
            format: ArgsFormat::Candid,
        }) => parse_idl_args(content.trim()).context(ParseInitArgsSnafu {
            canister,
            path: customize_path,
        }),
        Some(icp::InitArgs::Text { .. } | icp::InitArgs::Binary(_)) => {
            UnsupportedInitArgsFormatSnafu {
                canister,
                path: customize_path,
            }
            .fail()
        }
    }
}

/// One option resolved against the deploy: what to ask, and which canisters
/// receive the answer. Everything here is parsed before any prompt runs.
struct PlannedPrompt<'a> {
    option: &'a CustomizeOption,
    /// The option's canisters as written, for messages naming the option.
    canisters: String,
    /// The subset of them this deploy targets — never empty.
    targets: Vec<String>,
    plan: PlannedTarget,
}

/// A [`CustomizeTarget`] with everything the file spells as text already parsed.
enum PlannedTarget {
    InitArgField {
        field_path: FieldPath,
        type_env: TypeEnv,
        ty: candid::types::Type,
    },
    EnvVar {
        name: String,
    },
}

/// The answers one `--customize` run collected, keyed by canister store key. A
/// canister appears under `init_args` only once an option has customized a field
/// of its init args, and under `env_vars` only once one has customized a variable
/// of its own — the two are set through different calls and neither implies the
/// other.
#[derive(Debug, Default)]
pub(crate) struct Customizations {
    pub(crate) init_args: HashMap<String, IDLArgs>,
    pub(crate) env_vars: HashMap<String, BTreeMap<String, String>>,
}

impl Customizations {
    /// Lay a canister's answers over the environment variables it would
    /// otherwise deploy with, leaving every variable no option asked about as the
    /// manifest wrote it.
    pub(crate) fn overlay_env_vars(&self, canister: &str, settings: &mut Settings) {
        let Some(answers) = self.env_vars.get(canister) else {
            return;
        };
        settings
            .environment_variables
            .get_or_insert_default()
            .extend(answers.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
}

/// Substitute one answer into every canister its option applies to.
///
/// `result` holds each canister's args across the whole run, so several options
/// accumulate onto one canister and one option's answer reaches all of its
/// canisters. Every target is present before this is called.
fn apply_answer(
    result: &mut HashMap<String, IDLArgs>,
    targets: &[String],
    field_path: &FieldPath,
    value: IDLValue,
    customize_path: &Path,
) -> Result<(), PromptCustomizationsError> {
    for target in targets {
        let working_args = result
            .get_mut(target)
            .expect("every target's args are built before the prompts run");
        substitute_field(working_args, field_path, value.clone(), customize_path).context(
            SubstituteSnafu {
                canister: target,
                field_path: &field_path.text,
            },
        )?;
    }
    Ok(())
}

/// Ask for every option that applies to this deploy, returning what each canister
/// is to be customized with. Only called once the user has opted in with
/// `--customize`, so it always prompts.
pub(crate) fn prompt_customizations(
    manifest: &CustomizeManifest,
    cnames: &[String],
    init_args: &HashMap<String, Option<icp::InitArgs>>,
    customize_path: &Path,
) -> Result<Customizations, PromptCustomizationsError> {
    let cname_set: HashSet<&str> = cnames.iter().map(String::as_str).collect();

    // Resolve every option against this deploy, and parse everything that can be
    // rejected, before asking the first question — an unparseable candid_type or
    // an uncustomizable init_args format three options down must not surface
    // after the user has already typed answers that would then be thrown away.
    let mut planned: Vec<PlannedPrompt<'_>> = Vec::new();
    // A reference that resolves to no canister at all is rejected by
    // `validate_canister_refs`, so what is skipped here is only ever a canister
    // outside the current scope — worth reporting, since a member-scoped deploy
    // drops the rest of the workspace's options.
    let mut skipped: BTreeSet<String> = BTreeSet::new();

    for option in &manifest.options {
        let mut targets: Vec<String> = Vec::new();
        for reference in option.canister.iter() {
            let store_key = reference.store_key();
            match cname_set.contains(store_key.as_str()) {
                true => targets.push(store_key),
                false => {
                    skipped.insert(store_key);
                }
            }
        }
        // No canister here would receive the answer, so do not ask for it.
        if targets.is_empty() {
            continue;
        }

        let canisters = option.canister.joined();

        let plan = match &option.target {
            CustomizeTarget::InitArgField {
                field_path,
                candid_type,
            } => {
                let field_path = parse_field_path(field_path).context(FieldPathSnafu {
                    canisters: &canisters,
                    path: customize_path,
                })?;

                let (type_env, ty) =
                    parse_contextfree_candid_type_string(candid_type).context(CandidTypeSnafu {
                        canisters: &canisters,
                        field_path: &field_path.text,
                        path: customize_path,
                    })?;

                PlannedTarget::InitArgField {
                    field_path,
                    type_env,
                    ty,
                }
            }
            // Nothing left to parse: the name is validated as the file is read,
            // and the value is always text.
            CustomizeTarget::EnvVar { name } => PlannedTarget::EnvVar { name: name.clone() },
        };

        planned.push(PlannedPrompt {
            option,
            canisters,
            targets,
            plan,
        });
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

    let mut result = Customizations::default();

    // Each canister's args, accumulated across every option that names it. Built
    // here rather than on first substitution so an init_args format that cannot
    // be customized is caught before the prompts, not between them. Only the
    // canisters an init-arg option names are entered: an env-var option has no
    // use for its canister's init args, and must not be blocked by a format they
    // cannot be customized in.
    for prompt in &planned {
        if !matches!(prompt.plan, PlannedTarget::InitArgField { .. }) {
            continue;
        }
        for target in &prompt.targets {
            if !result.init_args.contains_key(target) {
                result.init_args.insert(
                    target.clone(),
                    manifest_init_args(init_args, target, customize_path)?,
                );
            }
        }
    }

    // One prompt per option, in the order the file lists them. An option naming
    // several canisters is asked once and its answer applied to each, so the
    // prompts cannot be grouped by canister — the file's order is the author's to
    // arrange.
    for prompt in &planned {
        eprintln!("[{}] {}", prompt.canisters, prompt.option.description);

        let prompt_snafu = PromptSnafu {
            canisters: &prompt.canisters,
            target: prompt.option.target.describe(),
            path: customize_path,
        };

        match &prompt.plan {
            PlannedTarget::InitArgField {
                field_path,
                type_env,
                ty,
            } => {
                let context = assist::Context::new(type_env.clone());
                let prompted =
                    assist::input_args(&context, std::slice::from_ref(ty)).context(prompt_snafu)?;

                let value = prompted
                    .args
                    .into_iter()
                    .next()
                    .expect("input_args returns one value per type element");

                apply_answer(
                    &mut result.init_args,
                    &prompt.targets,
                    field_path,
                    value,
                    customize_path,
                )?;
            }
            PlannedTarget::EnvVar { name } => {
                // A variable's value is a string, so the prompt is candid's plain
                // text input — the same one an init arg field of type `text` gets,
                // editor shortcut and all.
                let context = assist::Context::new(TypeEnv::new());
                let prompted = assist::input_args(&context, &[TypeInner::Text.into()])
                    .context(prompt_snafu)?;

                let IDLValue::Text(value) = prompted
                    .args
                    .into_iter()
                    .next()
                    .expect("input_args returns one value per type element")
                else {
                    unreachable!("a prompt for `text` yields text");
                };

                for target in &prompt.targets {
                    result
                        .env_vars
                        .entry(target.clone())
                        .or_default()
                        .insert(name.clone(), value.clone());
                }
            }
        }
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

    fn crefs(references: &[&str]) -> CanisterRefs {
        CanisterRefs(references.iter().copied().map(cref).collect())
    }

    fn option_for(reference: &str) -> CustomizeOption {
        options_for(&[reference])
    }

    fn options_for(references: &[&str]) -> CustomizeOption {
        CustomizeOption {
            canister: crefs(references),
            description: "desc".to_string(),
            target: CustomizeTarget::InitArgField {
                field_path: ".x".to_string(),
                candid_type: "nat64".to_string(),
            },
        }
    }

    fn env_option_for(reference: &str, name: &str) -> CustomizeOption {
        CustomizeOption {
            canister: crefs(&[reference]),
            description: "desc".to_string(),
            target: CustomizeTarget::EnvVar {
                name: name.to_string(),
            },
        }
    }

    /// The working-args map `prompt_customizations` builds before prompting.
    fn working(entries: &[(&str, &str)]) -> HashMap<String, IDLArgs> {
        entries
            .iter()
            .map(|(canister, args)| ((*canister).to_string(), parse_idl_args(args).expect("args")))
            .collect()
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
    fn substitute_passes_through_opt() {
        // `opt` is transparent in the path the same way a variant is: ".supply"
        // reaches the record inside the optional without a segment of its own.
        let mut args = parse_idl_args("(opt record { supply = 0 : nat64 })").unwrap();
        substitute_field(
            &mut args,
            &parse_field_path(".supply").unwrap(),
            IDLValue::Nat64(42),
            Path::new("test.yaml"),
        )
        .unwrap();

        let IDLValue::Opt(inner) = &args.args[0] else {
            panic!("expected opt, got {:?}", args.args[0]);
        };
        let IDLValue::Record(fields) = inner.as_ref() else {
            panic!("expected record inside the opt");
        };
        assert!(matches!(fields[0].val, IDLValue::Nat64(42)));
    }

    #[test]
    fn substitute_passes_through_opt_nested_in_record() {
        // An optional partway down the path is stepped over, not counted:
        // ".config.supply" spans record -> opt -> record.
        let mut args =
            parse_idl_args("(record { config = opt record { supply = 0 : nat64 } })").unwrap();
        substitute_field(
            &mut args,
            &parse_field_path(".config.supply").unwrap(),
            IDLValue::Nat64(42),
            Path::new("test.yaml"),
        )
        .unwrap();

        let IDLValue::Record(outer) = &args.args[0] else {
            panic!("expected record");
        };
        let IDLValue::Opt(inner) = &outer[0].val else {
            panic!("expected opt, got {:?}", outer[0].val);
        };
        let IDLValue::Record(fields) = inner.as_ref() else {
            panic!("expected record inside the opt");
        };
        assert!(matches!(fields[0].val, IDLValue::Nat64(42)));
    }

    #[test]
    fn substitute_into_absent_opt_err() {
        // An absent optional holds no payload to place a field in, so the path
        // cannot reach through it.
        let mut args = parse_idl_args("(record { config = null })").unwrap();
        let err = substitute_field(
            &mut args,
            &parse_field_path(".config.supply").unwrap(),
            IDLValue::Nat64(42),
            Path::new("test.yaml"),
        )
        .unwrap_err();
        assert!(matches!(err, SubstituteError::NotTraversable { .. }));
        assert!(err.to_string().contains("supply"), "got: {err}");
    }

    #[test]
    fn substitute_replaces_whole_opt() {
        // With no fields left to walk, the optional itself is what gets replaced —
        // which is how an absent one is filled in.
        let mut args = parse_idl_args("(record { config = null })").unwrap();
        substitute_field(
            &mut args,
            &parse_field_path(".config").unwrap(),
            IDLValue::Opt(Box::new(IDLValue::Nat64(42))),
            Path::new("test.yaml"),
        )
        .unwrap();

        let IDLValue::Record(outer) = &args.args[0] else {
            panic!("expected record");
        };
        let IDLValue::Opt(inner) = &outer[0].val else {
            panic!("expected opt, got {:?}", outer[0].val);
        };
        assert!(matches!(inner.as_ref(), IDLValue::Nat64(42)));
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
        assert_eq!(manifest.options[0].canister, crefs(&["my-canister"]));
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
            Path::new("icp_customize.yaml"),
        )
        .unwrap();
        assert!(result.init_args.is_empty() && result.env_vars.is_empty());
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
            crefs(&["services/open-crm:backend"])
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
    fn parse_refs_single_string() {
        let manifest: CustomizeManifest = serde_yaml::from_str(
            r#"
options:
  - canister: frontend
    field_path: ".title"
    candid_type: "text"
    description: "Site title"
"#,
        )
        .unwrap();
        assert_eq!(manifest.options[0].canister, crefs(&["frontend"]));
    }

    #[test]
    fn parse_refs_list() {
        let manifest: CustomizeManifest = serde_yaml::from_str(
            r#"
options:
  - canister: [frontend, "services/open-crm:backend"]
    field_path: ".admin"
    candid_type: "principal"
    description: "Administrator"
"#,
        )
        .unwrap();
        assert_eq!(
            manifest.options[0].canister,
            crefs(&["frontend", "services/open-crm:backend"])
        );
    }

    #[test]
    fn parse_refs_empty_list_err() {
        let err = serde_yaml::from_str::<CustomizeManifest>(
            r#"
options:
  - canister: []
    field_path: ".admin"
    candid_type: "principal"
    description: "Administrator"
"#,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("empty list"), "got: {msg}");
    }

    #[test]
    fn parse_refs_list_keeps_reference_error() {
        // The hand-written visitor must not flatten a bad reference inside a list
        // into a generic "did not match any variant".
        let err = serde_yaml::from_str::<CustomizeManifest>(
            r#"
options:
  - canister: [frontend, "/srv/open-crm:backend"]
    field_path: ".admin"
    candid_type: "principal"
    description: "Administrator"
"#,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("relative to the workspace root"), "got: {msg}");
    }

    #[test]
    fn apply_answer_reaches_every_canister() {
        // One answer, several canisters: each is substituted into its own args,
        // which start from its own manifest init_args.
        let mut result = working(&[
            ("frontend", "(record { supply = 0 : nat64 })"),
            (
                "services/open-crm:backend",
                "(record { supply = 7 : nat64 })",
            ),
        ]);
        let targets = vec![
            "frontend".to_string(),
            "services/open-crm:backend".to_string(),
        ];

        apply_answer(
            &mut result,
            &targets,
            &parse_field_path(".supply").unwrap(),
            IDLValue::Nat64(42),
            Path::new(CUSTOMIZE_FILE),
        )
        .unwrap();

        for target in &targets {
            let IDLValue::Record(fields) = &result[target].args[0] else {
                panic!("expected record for {target}");
            };
            assert!(
                matches!(fields[0].val, IDLValue::Nat64(42)),
                "{target} did not receive the answer: {:?}",
                fields[0].val
            );
        }
    }

    #[test]
    fn apply_answer_accumulates_across_options() {
        // A canister named by more than one option keeps the earlier answers: the
        // second option substitutes into the args the first one produced.
        let mut result = working(&[(
            "frontend",
            "(record { supply = 0 : nat64; limit = 0 : nat64 })",
        )]);
        let targets = vec!["frontend".to_string()];

        for (path, value) in [(".supply", 42u64), (".limit", 99)] {
            apply_answer(
                &mut result,
                &targets,
                &parse_field_path(path).unwrap(),
                IDLValue::Nat64(value),
                Path::new(CUSTOMIZE_FILE),
            )
            .unwrap();
        }

        let IDLValue::Record(fields) = &result["frontend"].args[0] else {
            panic!("expected record");
        };
        let value_of = |name: &str| {
            let id = Label::Named(name.to_string()).get_id();
            fields
                .iter()
                .find(|f| f.id.get_id() == id)
                .map(|f| f.val.clone())
        };
        assert!(matches!(value_of("supply"), Some(IDLValue::Nat64(42))));
        assert!(matches!(value_of("limit"), Some(IDLValue::Nat64(99))));
    }

    #[test]
    fn apply_answer_names_the_failing_canister() {
        // Only one of the option's canisters lacks the field, so the error has to
        // say which — the option itself names several.
        let mut result = working(&[
            ("frontend", "(record { supply = 0 : nat64 })"),
            (
                "services/open-crm:backend",
                "(record { other = 0 : nat64 })",
            ),
        ]);

        let err = apply_answer(
            &mut result,
            &[
                "frontend".to_string(),
                "services/open-crm:backend".to_string(),
            ],
            &parse_field_path(".supply").unwrap(),
            IDLValue::Nat64(42),
            Path::new(CUSTOMIZE_FILE),
        )
        .unwrap_err();

        assert!(matches!(err, PromptCustomizationsError::Substitute { .. }));
        let msg = err.to_string();
        assert!(msg.contains("services/open-crm:backend"), "got: {msg}");
        assert!(msg.contains(".supply"), "got: {msg}");
    }

    #[test]
    fn prompt_skips_option_whose_canisters_are_all_out_of_scope() {
        // Neither canister is being deployed, so there is nothing to apply an
        // answer to and no prompt fires — reaching one would hang the test.
        let manifest = CustomizeManifest {
            options: vec![options_for(&["a", "services/dep:b"])],
        };
        let result = prompt_customizations(
            &manifest,
            &["c".to_string()],
            &HashMap::new(),
            Path::new(CUSTOMIZE_FILE),
        )
        .unwrap();
        assert!(result.init_args.is_empty() && result.env_vars.is_empty());
    }

    #[test]
    fn parse_option_with_env_target() {
        let manifest: CustomizeManifest = serde_yaml::from_str(
            r#"
options:
  - canister: frontend
    env: API_ENDPOINT
    description: "Upstream API base URL"
"#,
        )
        .unwrap();
        assert_eq!(
            manifest.options[0].target,
            CustomizeTarget::EnvVar {
                name: "API_ENDPOINT".to_string()
            }
        );
    }

    /// The two targets are alternatives, and a half-written option names neither
    /// unambiguously. Each shape gets its own message, which an untagged enum
    /// would have flattened into "did not match any variant".
    #[test]
    fn parse_option_rejects_ambiguous_targets() {
        for (fields, expected) in [
            (
                "field_path: \".supply\"\n    candid_type: nat64\n    env: SUPPLY",
                "not both",
            ),
            ("field_path: \".supply\"", "no `candid_type`"),
            ("candid_type: nat64", "no `field_path`"),
            ("", "customizes nothing"),
        ] {
            let yaml = format!(
                "options:\n  - canister: frontend\n    description: d\n    {}\n",
                fields
            );
            let err = serde_yaml::from_str::<CustomizeManifest>(&yaml).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains(expected), "for {fields:?}, got: {msg}");
        }
    }

    #[test]
    fn parse_option_rejects_env_name_carrying_a_value() {
        // `env: KEY=value` is a shell assignment; the value is what the option
        // prompts for, so the name stands alone.
        let err = serde_yaml::from_str::<CustomizeManifest>(
            r#"
options:
  - canister: frontend
    env: "API_ENDPOINT=https://api.example.com"
    description: "Upstream API base URL"
"#,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("takes the name alone"), "got: {msg}");
    }

    #[test]
    fn parse_option_rejects_empty_env_name() {
        let err = serde_yaml::from_str::<CustomizeManifest>(
            r#"
options:
  - canister: frontend
    env: ""
    description: "Upstream API base URL"
"#,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("names no environment variable"),
            "got: {err}"
        );
    }

    #[test]
    fn parse_option_rejects_unknown_field() {
        // A misspelled target key would otherwise read as an option with no
        // target at all, blaming the wrong thing.
        let err = serde_yaml::from_str::<CustomizeManifest>(
            r#"
options:
  - canister: frontend
    env_var: API_ENDPOINT
    description: "Upstream API base URL"
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("env_var"), "got: {err}");
    }

    #[test]
    fn prompt_does_not_read_init_args_for_an_env_option() {
        // Init args are parsed up front for the canisters an init-arg option
        // names — an env-var option has no use for them, and a canister whose
        // init args cannot be customized is still free to have its environment
        // customized. Both canisters here have uncustomizable init args, so the
        // failure naming only "d" is what shows "c" was left alone.
        let manifest = CustomizeManifest {
            options: vec![env_option_for("c", "API_ENDPOINT"), option_for("d")],
        };
        let init_args = HashMap::from([
            ("c".to_string(), Some(icp::InitArgs::Binary(vec![0u8]))),
            ("d".to_string(), Some(icp::InitArgs::Binary(vec![0u8]))),
        ]);
        let err = prompt_customizations(
            &manifest,
            &["c".to_string(), "d".to_string()],
            &init_args,
            Path::new(CUSTOMIZE_FILE),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("\"d\""), "got: {msg}");
        assert!(!msg.contains("\"c\""), "got: {msg}");
    }

    #[test]
    fn overlay_env_vars_adds_to_a_canister_that_declared_none() {
        let customizations = Customizations {
            init_args: HashMap::new(),
            env_vars: HashMap::from([(
                "frontend".to_string(),
                BTreeMap::from([("API_ENDPOINT".to_string(), "https://api".to_string())]),
            )]),
        };
        let mut settings = Settings::default();
        customizations.overlay_env_vars("frontend", &mut settings);
        assert_eq!(
            settings.environment_variables,
            Some(HashMap::from([(
                "API_ENDPOINT".to_string(),
                "https://api".to_string()
            )]))
        );
    }

    #[test]
    fn overlay_env_vars_keeps_the_variables_no_option_asked_about() {
        let customizations = Customizations {
            init_args: HashMap::new(),
            env_vars: HashMap::from([(
                "frontend".to_string(),
                BTreeMap::from([("API_ENDPOINT".to_string(), "https://api".to_string())]),
            )]),
        };
        let mut settings = Settings {
            environment_variables: Some(HashMap::from([
                ("API_ENDPOINT".to_string(), "https://manifest".to_string()),
                ("LOG_LEVEL".to_string(), "debug".to_string()),
            ])),
            ..Settings::default()
        };
        customizations.overlay_env_vars("frontend", &mut settings);
        let vars = settings.environment_variables.unwrap();
        assert_eq!(vars["API_ENDPOINT"], "https://api");
        assert_eq!(vars["LOG_LEVEL"], "debug");
    }

    #[test]
    fn overlay_env_vars_leaves_an_unanswered_canister_alone() {
        // Not even an empty map: `sync_settings` reads `None` as "the manifest
        // says nothing about this canister's environment" and skips it, which is
        // not the same as "set it to nothing".
        let customizations = Customizations::default();
        let mut settings = Settings::default();
        customizations.overlay_env_vars("frontend", &mut settings);
        assert_eq!(settings.environment_variables, None);
    }

    #[test]
    fn validate_rejects_unknown_canister_in_a_list() {
        let manifest = CustomizeManifest {
            options: vec![options_for(&["frontend", "services/open_crm:backend"])],
        };
        let err = validate_canister_refs(
            &manifest,
            &["frontend", "services/open-crm:backend"],
            Path::new(CUSTOMIZE_FILE),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("services/open_crm:backend"),
            "got: {err}"
        );
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
