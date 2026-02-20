use std::collections::HashMap;

use candid::{Nat, Principal};
use icp_canister_interfaces::cycles_ledger::CanisterSettingsArg;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod build;
pub mod recipe;
pub mod sync;

mod script;

/// Controls who can read canister logs.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum LogVisibility {
    /// Only controllers can view logs.
    #[default]
    Controllers,
    /// Anyone can view logs.
    Public,
    /// Specific principals can view logs.
    AllowedViewers(Vec<Principal>),
}

/// Serialization/deserialization representation for LogVisibility.
/// Supports both string format ("controllers", "public") and object format ({ allowed_viewers: [...] }).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged, rename_all = "snake_case")]
enum LogVisibilityDef {
    /// Simple string variants for controllers or public
    Simple(LogVisibilitySimple),
    /// Object format with allowed_viewers list
    AllowedViewers { allowed_viewers: Vec<Principal> },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum LogVisibilitySimple {
    Controllers,
    Public,
}

impl From<LogVisibility> for LogVisibilityDef {
    fn from(value: LogVisibility) -> Self {
        match value {
            LogVisibility::Controllers => {
                LogVisibilityDef::Simple(LogVisibilitySimple::Controllers)
            }
            LogVisibility::Public => LogVisibilityDef::Simple(LogVisibilitySimple::Public),
            LogVisibility::AllowedViewers(viewers) => LogVisibilityDef::AllowedViewers {
                allowed_viewers: viewers,
            },
        }
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

impl Serialize for LogVisibility {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        LogVisibilityDef::from(self.clone()).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LogVisibility {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, MapAccess, Visitor};
        use std::fmt;

        struct LogVisibilityVisitor;

        impl<'de> Visitor<'de> for LogVisibilityVisitor {
            type Value = LogVisibility;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("'controllers', 'public', or object with 'allowed_viewers'")
            }

            fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
                LogVisibilityDef::deserialize(
                    serde::de::value::StrDeserializer::<E>::new(value),
                )
                .map(Into::into)
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
                    .map(LogVisibility::AllowedViewers)
                    .ok_or_else(|| Error::missing_field("allowed_viewers"))
            }
        }

        deserializer.deserialize_any(LogVisibilityVisitor)
    }
}

impl JsonSchema for LogVisibility {
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

impl From<LogVisibility> for ic_management_canister_types::LogVisibility {
    fn from(value: LogVisibility) -> Self {
        match value {
            LogVisibility::Controllers => ic_management_canister_types::LogVisibility::Controllers,
            LogVisibility::Public => ic_management_canister_types::LogVisibility::Public,
            LogVisibility::AllowedViewers(viewers) => {
                ic_management_canister_types::LogVisibility::AllowedViewers(viewers)
            }
        }
    }
}

impl From<LogVisibility> for icp_canister_interfaces::cycles_ledger::LogVisibility {
    fn from(value: LogVisibility) -> Self {
        use icp_canister_interfaces::cycles_ledger::LogVisibility as CyclesLedgerLogVisibility;
        match value {
            LogVisibility::Controllers => CyclesLedgerLogVisibility::Controllers,
            LogVisibility::Public => CyclesLedgerLogVisibility::Public,
            LogVisibility::AllowedViewers(viewers) => {
                CyclesLedgerLogVisibility::AllowedViewers(viewers)
            }
        }
    }
}

/// Canister settings, such as compute and memory allocation.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Settings {
    /// Controls who can read canister logs.
    pub log_visibility: Option<LogVisibility>,

    /// Compute allocation (0 to 100). Represents guaranteed compute capacity.
    pub compute_allocation: Option<u64>,

    /// Memory allocation in bytes. If unset, memory is allocated dynamically.
    pub memory_allocation: Option<u64>,

    /// Freezing threshold in seconds. Controls how long a canister can be inactive before being frozen.
    pub freezing_threshold: Option<u64>,

    /// Upper limit on cycles reserved for future resource payments.
    /// Memory allocations that would push the reserved balance above this limit will fail.
    /// Supports suffixes in YAML: k, m, b, t (e.g. "4t" or "4.3t").
    #[serde(default)]
    pub reserved_cycles_limit: Option<crate::parsers::CyclesAmount>,

    /// Wasm memory limit in bytes. Sets an upper bound for Wasm heap growth.
    pub wasm_memory_limit: Option<u64>,

    /// Wasm memory threshold in bytes. Triggers a callback when exceeded.
    pub wasm_memory_threshold: Option<u64>,

    /// Environment variables for the canister as key-value pairs.
    /// These variables are accessible within the canister and can be used to configure
    /// behavior without hardcoding values in the WASM module.
    pub environment_variables: Option<HashMap<String, String>>,
}

impl From<Settings> for CanisterSettingsArg {
    fn from(settings: Settings) -> Self {
        CanisterSettingsArg {
            freezing_threshold: settings.freezing_threshold.map(Nat::from),
            controllers: None,
            reserved_cycles_limit: settings.reserved_cycles_limit.map(|c| Nat::from(c.get())),
            log_visibility: settings.log_visibility.map(Into::into),
            memory_allocation: settings.memory_allocation.map(Nat::from),
            compute_allocation: settings.compute_allocation.map(Nat::from),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_visibility_deserialize_controllers() {
        let yaml = "controllers";
        let result: LogVisibility = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(result, LogVisibility::Controllers);
    }

    #[test]
    fn log_visibility_deserialize_public() {
        let yaml = "public";
        let result: LogVisibility = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(result, LogVisibility::Public);
    }

    #[test]
    fn log_visibility_deserialize_allowed_viewers() {
        let yaml = r#"
allowed_viewers:
  - "aaaaa-aa"
  - "2vxsx-fae"
"#;
        let result: LogVisibility = serde_yaml::from_str(yaml).unwrap();
        match result {
            LogVisibility::AllowedViewers(viewers) => {
                assert_eq!(viewers.len(), 2);
                assert_eq!(viewers[0], Principal::from_text("aaaaa-aa").unwrap());
                assert_eq!(viewers[1], Principal::from_text("2vxsx-fae").unwrap());
            }
            _ => panic!("Expected AllowedViewers variant"),
        }
    }

    #[test]
    fn log_visibility_deserialize_allowed_viewers_empty() {
        let yaml = "allowed_viewers: []";
        let result: LogVisibility = serde_yaml::from_str(yaml).unwrap();
        match result {
            LogVisibility::AllowedViewers(viewers) => {
                assert!(viewers.is_empty());
            }
            _ => panic!("Expected AllowedViewers variant"),
        }
    }

    #[test]
    fn log_visibility_deserialize_invalid_string() {
        let yaml = "invalid";
        let result: Result<LogVisibility, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown log_visibility value"));
    }

    #[test]
    fn log_visibility_deserialize_invalid_field() {
        let yaml = "unknown_field: []";
        let result: Result<LogVisibility, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown field"));
    }

    #[test]
    fn log_visibility_serialize_controllers() {
        let log_vis = LogVisibility::Controllers;
        let yaml = serde_yaml::to_string(&log_vis).unwrap();
        assert_eq!(yaml.trim(), "controllers");
    }

    #[test]
    fn log_visibility_serialize_public() {
        let log_vis = LogVisibility::Public;
        let yaml = serde_yaml::to_string(&log_vis).unwrap();
        assert_eq!(yaml.trim(), "public");
    }

    #[test]
    fn log_visibility_serialize_allowed_viewers() {
        let log_vis = LogVisibility::AllowedViewers(vec![
            Principal::from_text("aaaaa-aa").unwrap(),
            Principal::from_text("2vxsx-fae").unwrap(),
        ]);
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
    fn log_visibility_conversion_to_ic_type() {
        let controllers = LogVisibility::Controllers;
        let ic_controllers: ic_management_canister_types::LogVisibility = controllers.into();
        assert!(matches!(
            ic_controllers,
            ic_management_canister_types::LogVisibility::Controllers
        ));

        let public = LogVisibility::Public;
        let ic_public: ic_management_canister_types::LogVisibility = public.into();
        assert!(matches!(
            ic_public,
            ic_management_canister_types::LogVisibility::Public
        ));

        let viewers =
            LogVisibility::AllowedViewers(vec![Principal::from_text("aaaaa-aa").unwrap()]);
        let ic_viewers: ic_management_canister_types::LogVisibility = viewers.into();
        match ic_viewers {
            ic_management_canister_types::LogVisibility::AllowedViewers(v) => {
                assert_eq!(v.len(), 1);
            }
            _ => panic!("Expected AllowedViewers"),
        }
    }
}
