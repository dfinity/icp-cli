use std::collections::HashMap;

use candid::{Nat, Principal};
use ic_management_canister_types::{CanisterSettings, LogVisibility};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::parsers::{CyclesAmount, DurationAmount, MemoryAmount};

pub mod build;
pub mod recipe;
pub mod sync;

mod script;
mod wasm;

/// Controls who can read canister logs.
/// Supports both string format ("controllers", "public") and object format ({ allowed_viewers: [...] }).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum LogVisibilityDef {
    /// Simple string variants for controllers or public
    Simple(LogVisibilitySimple),
    /// Object format with allowed_viewers list
    AllowedViewers { allowed_viewers: Vec<Principal> },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogVisibilitySimple {
    Controllers,
    Public,
}

impl<'de> Deserialize<'de> for LogVisibilityDef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, MapAccess, Visitor};
        use std::fmt;

        struct LogVisibilityVisitor;

        impl<'de> Visitor<'de> for LogVisibilityVisitor {
            type Value = LogVisibilityDef;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("'controllers', 'public', or object with 'allowed_viewers'")
            }

            fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
                LogVisibilitySimple::deserialize(
                    serde::de::value::StrDeserializer::<E>::new(value),
                )
                .map(LogVisibilityDef::Simple)
                .map_err(|_| {
                    E::custom(format!(
                        "unknown log_visibility value: '{}', expected 'controllers' or 'public'",
                        value
                    ))
                })
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut allowed_viewers: Option<Vec<Principal>> = None;

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "allowed_viewers" => {
                            if allowed_viewers.is_some() {
                                return Err(Error::duplicate_field("allowed_viewers"));
                            }
                            allowed_viewers = Some(map.next_value()?);
                        }
                        _ => {
                            return Err(Error::unknown_field(&key, &["allowed_viewers"]));
                        }
                    }
                }

                allowed_viewers
                    .map(|v| LogVisibilityDef::AllowedViewers { allowed_viewers: v })
                    .ok_or_else(|| Error::missing_field("allowed_viewers"))
            }
        }

        deserializer.deserialize_any(LogVisibilityVisitor)
    }
}

impl JsonSchema for LogVisibilityDef {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("LogVisibility")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "Controls who can read canister logs.",
            "oneOf": [
                {
                    "type": "string",
                    "enum": ["controllers", "public"],
                    "description": "Simple log visibility: 'controllers' (only controllers can view) or 'public' (anyone can view)"
                },
                {
                    "type": "object",
                    "properties": {
                        "allowed_viewers": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "description": "A principal ID that can view logs"
                            },
                            "description": "List of principal IDs that can view canister logs"
                        }
                    },
                    "required": ["allowed_viewers"],
                    "additionalProperties": false,
                    "description": "Specific principals that can view logs"
                }
            ]
        })
    }
}

impl From<LogVisibilityDef> for LogVisibility {
    fn from(value: LogVisibilityDef) -> Self {
        match value {
            LogVisibilityDef::Simple(LogVisibilitySimple::Controllers) => {
                LogVisibility::Controllers
            }
            LogVisibilityDef::Simple(LogVisibilitySimple::Public) => LogVisibility::Public,
            LogVisibilityDef::AllowedViewers { allowed_viewers } => {
                LogVisibility::AllowedViewers(allowed_viewers)
            }
        }
    }
}

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

/// Canister settings, such as compute and memory allocation.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Settings {
    /// Controls who can read canister logs.
    pub log_visibility: Option<LogVisibilityDef>,

    /// Compute allocation (0 to 100). Represents guaranteed compute capacity.
    pub compute_allocation: Option<u64>,

    /// Memory allocation in bytes. If unset, memory is allocated dynamically.
    /// Supports suffixes in YAML: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb").
    pub memory_allocation: Option<MemoryAmount>,

    /// Freezing threshold in seconds. Controls how long a canister can be inactive before being frozen.
    /// Supports duration suffixes in YAML: s, m, h, d, w (e.g. "30d" or "4w").
    pub freezing_threshold: Option<DurationAmount>,

    /// Upper limit on cycles reserved for future resource payments.
    /// Memory allocations that would push the reserved balance above this limit will fail.
    /// Supports suffixes in YAML: k, m, b, t (e.g. "4t" or "4.3t").
    #[serde(default)]
    pub reserved_cycles_limit: Option<CyclesAmount>,

    /// Wasm memory limit in bytes. Sets an upper bound for Wasm heap growth.
    /// Supports suffixes in YAML: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb").
    pub wasm_memory_limit: Option<MemoryAmount>,

    /// Wasm memory threshold in bytes. Triggers a callback when exceeded.
    /// Supports suffixes in YAML: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb").
    pub wasm_memory_threshold: Option<MemoryAmount>,

    /// Log memory limit in bytes (max 2 MiB). Oldest logs are purged when usage exceeds this value.
    /// Supports suffixes in YAML: kb, kib, mb, mib (e.g. "2mib" or "256kib"). Canister default is 4096 bytes.
    pub log_memory_limit: Option<MemoryAmount>,

    /// Environment variables for the canister as key-value pairs.
    /// These variables are accessible within the canister and can be used to configure
    /// behavior without hardcoding values in the WASM module.
    pub environment_variables: Option<HashMap<String, String>>,

    /// Controllers for this canister. Each entry is either a principal text
    /// (e.g. "2vxsx-fae") or the name of another canister in this project.
    /// Named canisters that do not yet exist will be set as controllers once created.
    #[serde(default)]
    pub controllers: Option<Vec<ControllerRef>>,
}

impl From<Settings> for CanisterSettings {
    fn from(settings: Settings) -> Self {
        CanisterSettings {
            freezing_threshold: settings.freezing_threshold.map(|d| Nat::from(d.get())),
            controllers: None,
            reserved_cycles_limit: settings.reserved_cycles_limit.map(|c| Nat::from(c.get())),
            log_visibility: settings.log_visibility.map(Into::into),
            memory_allocation: settings.memory_allocation.map(|m| Nat::from(m.get())),
            compute_allocation: settings.compute_allocation.map(Nat::from),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_visibility_deserialize_controllers() {
        let yaml = "controllers";
        let result: LogVisibilityDef = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            result,
            LogVisibilityDef::Simple(LogVisibilitySimple::Controllers)
        );
    }

    #[test]
    fn log_visibility_deserialize_public() {
        let yaml = "public";
        let result: LogVisibilityDef = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            result,
            LogVisibilityDef::Simple(LogVisibilitySimple::Public)
        );
    }

    #[test]
    fn log_visibility_deserialize_allowed_viewers() {
        let yaml = r#"
allowed_viewers:
  - "aaaaa-aa"
  - "2vxsx-fae"
"#;
        let result: LogVisibilityDef = serde_yaml::from_str(yaml).unwrap();
        match result {
            LogVisibilityDef::AllowedViewers { allowed_viewers } => {
                assert_eq!(allowed_viewers.len(), 2);
                assert_eq!(
                    allowed_viewers[0],
                    Principal::from_text("aaaaa-aa").unwrap()
                );
                assert_eq!(
                    allowed_viewers[1],
                    Principal::from_text("2vxsx-fae").unwrap()
                );
            }
            _ => panic!("Expected AllowedViewers variant"),
        }
    }

    #[test]
    fn log_visibility_deserialize_allowed_viewers_empty() {
        let yaml = "allowed_viewers: []";
        let result: LogVisibilityDef = serde_yaml::from_str(yaml).unwrap();
        match result {
            LogVisibilityDef::AllowedViewers { allowed_viewers } => {
                assert!(allowed_viewers.is_empty());
            }
            _ => panic!("Expected AllowedViewers variant"),
        }
    }

    #[test]
    fn log_visibility_deserialize_invalid_string() {
        let yaml = "invalid";
        let result: Result<LogVisibilityDef, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown log_visibility value"));
    }

    #[test]
    fn log_visibility_deserialize_invalid_field() {
        let yaml = "unknown_field: []";
        let result: Result<LogVisibilityDef, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown field"));
    }

    #[test]
    fn log_visibility_serialize_controllers() {
        let log_vis = LogVisibilityDef::Simple(LogVisibilitySimple::Controllers);
        let yaml = serde_yaml::to_string(&log_vis).unwrap();
        assert_eq!(yaml.trim(), "controllers");
    }

    #[test]
    fn log_visibility_serialize_public() {
        let log_vis = LogVisibilityDef::Simple(LogVisibilitySimple::Public);
        let yaml = serde_yaml::to_string(&log_vis).unwrap();
        assert_eq!(yaml.trim(), "public");
    }

    #[test]
    fn log_visibility_serialize_allowed_viewers() {
        let log_vis = LogVisibilityDef::AllowedViewers {
            allowed_viewers: vec![
                Principal::from_text("aaaaa-aa").unwrap(),
                Principal::from_text("2vxsx-fae").unwrap(),
            ],
        };
        let yaml = serde_yaml::to_string(&log_vis).unwrap();
        assert!(yaml.contains("allowed_viewers"));
        assert!(yaml.contains("aaaaa-aa"));
        assert!(yaml.contains("2vxsx-fae"));
    }

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

    #[test]
    fn log_visibility_conversion_to_ic_type() {
        let controllers = LogVisibilityDef::Simple(LogVisibilitySimple::Controllers);
        let ic_controllers: LogVisibility = controllers.into();
        assert!(matches!(ic_controllers, LogVisibility::Controllers));

        let public = LogVisibilityDef::Simple(LogVisibilitySimple::Public);
        let ic_public: LogVisibility = public.into();
        assert!(matches!(ic_public, LogVisibility::Public));

        let viewers = LogVisibilityDef::AllowedViewers {
            allowed_viewers: vec![Principal::from_text("aaaaa-aa").unwrap()],
        };
        let ic_viewers: LogVisibility = viewers.into();
        match ic_viewers {
            LogVisibility::AllowedViewers(v) => {
                assert_eq!(v.len(), 1);
            }
            _ => panic!("Expected AllowedViewers"),
        }
    }
}
