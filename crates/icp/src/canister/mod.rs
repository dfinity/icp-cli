use std::collections::HashMap;

use candid::Nat;
use ic_management_canister_types::CanisterSettings;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    parsers::{CyclesAmount, DurationAmount, MemoryAmount},
    prelude::*,
};

pub mod build;
pub mod recipe;
pub mod sync;
pub mod visibility;

mod script;
pub mod wasm;

pub use visibility::{LogVisibilityDef, StatusVisibilityDef, Visibility};

/// A reference to a controller: either an explicit principal or a canister name in this project.
///
/// During deserialization, principal text format is tried first; strings that don't parse as a
/// principal are treated as canister names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ControllerRef {
    /// An explicitly specified principal (e.g. "2vxsx-fae")
    Principal(candid::Principal),
    /// A canister name from the same project (e.g. "my_canister")
    CanisterName(String),
}

impl ControllerRef {
    /// Resolve to a `Principal` using the provided ID mapping.
    /// Returns `None` if this is a `CanisterName` not present in `ids`.
    pub fn resolve(&self, ids: &crate::store_id::IdMapping) -> Option<candid::Principal> {
        match self {
            ControllerRef::Principal(p) => Some(*p),
            ControllerRef::CanisterName(name) => ids.get(name).copied(),
        }
    }

    /// If this is a `CanisterName`, returns the name; otherwise `None`.
    pub fn canister_name(&self) -> Option<&str> {
        match self {
            ControllerRef::CanisterName(n) => Some(n),
            ControllerRef::Principal(_) => None,
        }
    }
}

/// Partition a slice of controller references into resolved principals and unresolved canister
/// names, using `ids` for name lookup.
pub fn resolve_controllers(
    crefs: &[ControllerRef],
    ids: &crate::store_id::IdMapping,
) -> (Vec<candid::Principal>, Vec<String>) {
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();
    for cref in crefs {
        match cref.resolve(ids) {
            Some(p) => resolved.push(p),
            None => {
                if let Some(name) = cref.canister_name() {
                    unresolved.push(name.to_owned());
                }
            }
        }
    }
    (resolved, unresolved)
}

impl schemars::JsonSchema for ControllerRef {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("ControllerRef")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "A controller: either a principal text (e.g. '2vxsx-fae') or a canister name in this project (e.g. 'my_canister')"
        })
    }
}

/// An environment variable value as written in a manifest.
///
/// A plain scalar is the value itself:
/// ```yaml
/// environment_variables:
///   API_ENDPOINT: https://api.example.com
/// ```
///
/// The object form reads the value from a file, relative to the canister's own
/// directory — including when an environment overrides the variable, matching how
/// an `init_args` override resolves its path:
/// ```yaml
/// environment_variables:
///   API_KEY:
///     path: ./secrets/api-key
/// ```
#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema, Serialize)]
#[serde(untagged, expecting = "a string, or `{ path: <file> }`")]
pub enum ManifestEnvVar {
    /// The value, written inline.
    Value(String),
    /// A file holding the value. Surrounding whitespace is trimmed off the
    /// file's contents, so a trailing newline does not become part of the value.
    Path {
        #[schemars(with = "String")]
        path: PathBuf,
    },
}

impl Default for ManifestEnvVar {
    fn default() -> Self {
        Self::Value(String::new())
    }
}

/// Canister settings loaded from a manifest, before file-backed environment
/// variable values have been read. See [`Settings`] for the resolved form.
pub type ManifestSettings = Settings<ManifestEnvVar>;

/// Canister settings, such as compute and memory allocation.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Settings<EnvVar = String> {
    /// Controls who can read canister logs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_visibility: Option<LogVisibilityDef>,

    /// Controls who can read the canister's status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_visibility: Option<StatusVisibilityDef>,

    /// Compute allocation (0 to 100). Represents guaranteed compute capacity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compute_allocation: Option<u64>,

    /// Memory allocation in bytes. If unset, memory is allocated dynamically.
    /// Supports suffixes in YAML: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_allocation: Option<MemoryAmount>,

    /// Freezing threshold in seconds. Controls how long a canister can be inactive before being frozen.
    /// Supports duration suffixes in YAML: s, m, h, d, w (e.g. "30d" or "4w").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freezing_threshold: Option<DurationAmount>,

    /// Upper limit on cycles reserved for future resource payments.
    /// Memory allocations that would push the reserved balance above this limit will fail.
    /// Supports suffixes in YAML: k, m, b, t (e.g. "4t" or "4.3t").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved_cycles_limit: Option<CyclesAmount>,

    /// Wasm memory limit in bytes. Sets an upper bound for Wasm heap growth.
    /// Supports suffixes in YAML: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wasm_memory_limit: Option<MemoryAmount>,

    /// Wasm memory threshold in bytes. Triggers a callback when exceeded.
    /// Supports suffixes in YAML: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wasm_memory_threshold: Option<MemoryAmount>,

    /// Log memory limit in bytes (max 2 MiB). Oldest logs are purged when usage exceeds this value.
    /// Supports suffixes in YAML: kb, kib, mb, mib (e.g. "2mib" or "256kib"). Canister default is 4096 bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_memory_limit: Option<MemoryAmount>,

    /// Environment variables for the canister as key-value pairs.
    /// These variables are accessible within the canister and can be used to configure
    /// behavior without hardcoding values in the WASM module.
    /// A value may also be read from a file with `{ path: <file> }`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_variables: Option<HashMap<String, EnvVar>>,

    /// Controllers for this canister. Each entry is either a principal text
    /// (e.g. "2vxsx-fae") or the name of another canister in this project.
    /// Named canisters that do not yet exist will be set as controllers once created.
    #[serde(default)]
    pub controllers: Option<Vec<ControllerRef>>,
}

impl From<Settings> for ManifestSettings {
    fn from(settings: Settings) -> Self {
        let Settings {
            log_visibility,
            status_visibility,
            compute_allocation,
            memory_allocation,
            freezing_threshold,
            reserved_cycles_limit,
            wasm_memory_limit,
            wasm_memory_threshold,
            log_memory_limit,
            environment_variables,
            controllers,
        } = settings;

        Self {
            log_visibility,
            status_visibility,
            compute_allocation,
            memory_allocation,
            freezing_threshold,
            reserved_cycles_limit,
            wasm_memory_limit,
            wasm_memory_threshold,
            log_memory_limit,
            environment_variables: environment_variables.map(|vars| {
                vars.into_iter()
                    .map(|(name, value)| (name, ManifestEnvVar::Value(value)))
                    .collect()
            }),
            controllers,
        }
    }
}

impl From<Settings> for CanisterSettings {
    fn from(settings: Settings) -> Self {
        CanisterSettings {
            freezing_threshold: settings.freezing_threshold.map(|d| Nat::from(d.get())),
            controllers: None,
            reserved_cycles_limit: settings.reserved_cycles_limit.map(|c| Nat::from(c.get())),
            log_visibility: settings.log_visibility.map(|v| v.0.into()),
            status_visibility: settings.status_visibility.map(|v| v.0.into()),
            memory_allocation: settings.memory_allocation.map(|m| Nat::from(m.get())),
            compute_allocation: settings.compute_allocation.map(Nat::from),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use candid::Principal;
    use indoc::indoc;

    use super::*;

    #[test]
    fn settings_reserved_cycles_limit_parses_suffix() {
        let yaml = "reserved_cycles_limit: 4.3t";
        let settings: Settings = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            settings.reserved_cycles_limit.as_ref().map(|c| c.get()),
            Some(4_300_000_000_000)
        );
    }

    #[test]
    fn settings_reserved_cycles_limit_parses_number() {
        let yaml = "reserved_cycles_limit: 5000000000000";
        let settings: Settings = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            settings.reserved_cycles_limit.as_ref().map(|c| c.get()),
            Some(5_000_000_000_000)
        );
    }

    #[test]
    fn settings_memory_allocation_parses_suffix() {
        let yaml = "memory_allocation: 4gib";
        let settings: Settings = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            settings.memory_allocation.as_ref().map(|m| m.get()),
            Some(4 * 1024 * 1024 * 1024)
        );
    }

    #[test]
    fn settings_memory_allocation_parses_number() {
        let yaml = "memory_allocation: 4294967296";
        let settings: Settings = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            settings.memory_allocation.as_ref().map(|m| m.get()),
            Some(4294967296)
        );
    }

    #[test]
    fn settings_wasm_memory_limit_parses_suffix() {
        let yaml = "wasm_memory_limit: 1.5gib";
        let settings: Settings = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            settings.wasm_memory_limit.as_ref().map(|m| m.get()),
            Some(1610612736)
        );
    }

    #[test]
    fn settings_log_memory_limit_parses_suffix() {
        let yaml = "log_memory_limit: 256kib";
        let settings: Settings = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            settings.log_memory_limit.as_ref().map(|m| m.get()),
            Some(256 * 1024)
        );
    }

    #[test]
    fn settings_log_memory_limit_parses_mib() {
        let yaml = "log_memory_limit: 2mib";
        let settings: Settings = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            settings.log_memory_limit.as_ref().map(|m| m.get()),
            Some(2 * 1024 * 1024)
        );
    }

    #[test]
    fn settings_environment_variables_take_values_or_files() {
        let yaml = indoc! {r#"
            environment_variables:
              API_ENDPOINT: https://api.example.com
              API_KEY:
                path: ./secrets/api-key
        "#};
        let settings: ManifestSettings = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            settings.environment_variables,
            Some(HashMap::from([
                (
                    "API_ENDPOINT".to_owned(),
                    ManifestEnvVar::Value("https://api.example.com".to_owned()),
                ),
                (
                    "API_KEY".to_owned(),
                    ManifestEnvVar::Path {
                        path: "./secrets/api-key".into(),
                    },
                ),
            ])),
        );
    }

    #[test]
    fn settings_environment_variable_rejects_unknown_object_form() {
        let yaml = indoc! {r#"
            environment_variables:
              API_KEY:
                file: ./secrets/api-key
        "#};
        let err = serde_yaml::from_str::<ManifestSettings>(yaml)
            .expect_err("only the `path` object form is accepted");
        assert!(
            err.to_string().contains("a string, or `{ path: <file> }`"),
            "unhelpful error: {err}"
        );
    }

    /// A value of the wrong scalar type reports what is accepted, rather than
    /// serde's default "did not match any variant" for an untagged enum.
    #[test]
    fn settings_environment_variable_rejects_non_string_scalar() {
        let err =
            serde_yaml::from_str::<ManifestSettings>("environment_variables:\n  PORT: 8080\n")
                .expect_err("a bare integer is not a value");
        assert!(
            err.to_string().contains("a string, or `{ path: <file> }`"),
            "unhelpful error: {err}"
        );
    }

    #[test]
    fn resolved_settings_serialize_environment_variables_inline() {
        let settings = Settings {
            environment_variables: Some(HashMap::from([(
                "API_KEY".to_owned(),
                "s3cret".to_owned(),
            )])),
            ..Default::default()
        };
        let yaml = serde_yaml::to_string(&ManifestSettings::from(settings)).unwrap();
        assert!(
            yaml.contains("environment_variables:\n  API_KEY: s3cret\n"),
            "unexpected yaml: {yaml}"
        );
    }

    #[test]
    fn controller_ref_deserializes_principal() {
        let yaml = "\"2vxsx-fae\"";
        let result: ControllerRef = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            result,
            ControllerRef::Principal(Principal::from_text("2vxsx-fae").unwrap())
        );
    }

    #[test]
    fn controller_ref_deserializes_canister_name() {
        let yaml = "\"my_canister\"";
        let result: ControllerRef = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            result,
            ControllerRef::CanisterName("my_canister".to_owned())
        );
    }

    #[test]
    fn controller_ref_resolve_principal() {
        let p = Principal::from_text("aaaaa-aa").unwrap();
        let cref = ControllerRef::Principal(p);
        let ids = crate::store_id::IdMapping::new();
        assert_eq!(cref.resolve(&ids), Some(p));
    }

    #[test]
    fn controller_ref_resolve_canister_name_present() {
        let p = Principal::from_text("aaaaa-aa").unwrap();
        let cref = ControllerRef::CanisterName("backend".to_owned());
        let mut ids = crate::store_id::IdMapping::new();
        ids.insert("backend".to_owned(), p);
        assert_eq!(cref.resolve(&ids), Some(p));
    }

    #[test]
    fn controller_ref_resolve_canister_name_absent() {
        let cref = ControllerRef::CanisterName("backend".to_owned());
        let ids = crate::store_id::IdMapping::new();
        assert_eq!(cref.resolve(&ids), None);
    }

    #[test]
    fn settings_controllers_parses_mixed() {
        let yaml = r#"
controllers:
  - "aaaaa-aa"
  - "my_other_canister"
"#;
        let settings: Settings = serde_yaml::from_str(yaml).unwrap();
        let controllers = settings.controllers.unwrap();
        assert_eq!(controllers.len(), 2);
        assert_eq!(
            controllers[0],
            ControllerRef::Principal(Principal::from_text("aaaaa-aa").unwrap())
        );
        assert_eq!(
            controllers[1],
            ControllerRef::CanisterName("my_other_canister".to_owned())
        );
    }
}
