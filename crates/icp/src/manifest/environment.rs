use std::{collections::HashMap, fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, Error as _},
};
use snafu::prelude::*;

use crate::{canister::ManifestSettings, prelude::LOCAL};

use super::canister::ManifestInitArgs;

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
pub struct EnvironmentInner {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canisters: Option<Vec<String>>,
    #[serde(
        rename = "exclude-canisters",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub exclude_canisters: Option<Vec<CanisterExclusion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<HashMap<String, ManifestSettings>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub init_args: Option<HashMap<String, ManifestInitArgs>>,
}

/// One entry of an environment's `exclude-canisters:` list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanisterExclusion {
    /// A single canister, named the way the rest of the manifest names it: a
    /// bare local name for one of this project's own canisters, or
    /// `subproject:canister` for a canister of a subproject.
    Canister(String),

    /// A whole subproject, written as its key followed by `:` with no canister
    /// name. Covers the subproject's own canisters and those of the subprojects
    /// nested beneath it.
    Subproject(String),
}

impl CanisterExclusion {
    /// Whether this entry excludes the canister with the store key `key`.
    pub fn matches(&self, key: &str) -> bool {
        match self {
            Self::Canister(name) => key == name,
            // `<prefix>:<canister>` for the subproject's own canisters,
            // `<prefix>/<nested>:<canister>` for one nested beneath it.
            Self::Subproject(prefix) => key
                .strip_prefix(prefix.as_str())
                .is_some_and(|rest| rest.starts_with(':') || rest.starts_with('/')),
        }
    }
}

#[derive(Debug, Snafu)]
pub enum ParseCanisterExclusionError {
    #[snafu(display("a canister exclusion may not be empty"))]
    Empty,

    #[snafu(display("`:` is not a subproject: name the subproject before it, as `subproject:`"))]
    EmptySubproject,
}

impl FromStr for CanisterExclusion {
    type Err = ParseCanisterExclusionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.strip_suffix(':') {
            Some(prefix) => match prefix.is_empty() {
                true => EmptySubprojectSnafu.fail(),
                false => Ok(Self::Subproject(prefix.to_owned())),
            },
            None => match s.is_empty() {
                true => EmptySnafu.fail(),
                false => Ok(Self::Canister(s.to_owned())),
            },
        }
    }
}

impl fmt::Display for CanisterExclusion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Canister(name) => f.write_str(name),
            Self::Subproject(prefix) => write!(f, "{prefix}:"),
        }
    }
}

impl<'de> Deserialize<'de> for CanisterExclusion {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(CanisterExclusionVisitor)
    }
}

struct CanisterExclusionVisitor;

impl<'de> de::Visitor<'de> for CanisterExclusionVisitor {
    type Value = CanisterExclusion;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a canister name, or a subproject written as `subproject:`")
    }

    fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
        s.parse().map_err(E::custom)
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        // An unquoted `subproject:` list entry is a one-key mapping with no
        // value as far as YAML is concerned, so accept that spelling instead of
        // making the quotes mandatory.
        let Some((key, value)) = map.next_entry::<String, Option<de::IgnoredAny>>()? else {
            return Err(A::Error::custom(
                "a subproject exclusion must name the subproject, as `subproject:`",
            ));
        };
        if value.is_some() {
            return Err(A::Error::custom(format!(
                "'{key}:' excludes the whole subproject and takes no value"
            )));
        }
        if map
            .next_entry::<de::IgnoredAny, de::IgnoredAny>()?
            .is_some()
        {
            return Err(A::Error::custom(
                "each excluded subproject needs its own list entry",
            ));
        }
        format!("{key}:").parse().map_err(A::Error::custom)
    }
}

impl Serialize for CanisterExclusion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl JsonSchema for CanisterExclusion {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("CanisterExclusion")
    }

    fn json_schema(_generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "description": "A canister to exclude: a canister name as used elsewhere in the project (e.g. 'backend' or 'services/crm:backend'), or a subproject and everything nested beneath it, written with a trailing ':' and no canister name (e.g. 'services/crm:')",
            "anyOf": [
                { "type": "string" },
                // An unquoted `subproject:` entry, which YAML reads as a
                // one-key mapping with no value.
                {
                    "type": "object",
                    "minProperties": 1,
                    "maxProperties": 1,
                    "additionalProperties": { "type": "null" }
                }
            ]
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema)]
pub enum CanisterSelection {
    /// No canisters are selected.
    None,

    /// A specific list of canisters is selected by name.
    /// An empty list is allowed, but `None` is preferred to indicate no selection.
    Named(Vec<String>),

    /// All canisters are selected.
    /// This is the default variant.
    #[default]
    Everything,
}

#[derive(Clone, Debug, PartialEq, JsonSchema)]
pub struct EnvironmentManifest {
    // The environment name
    pub name: String,

    /// The target network for canister deployment.
    /// Defaults to the `local` network if not specified
    #[schemars(with = "Option<String>")]
    pub network: String,

    /// An optional list of the canisters to be included in this environments.
    /// Defaults to all the canisters if not specified.
    #[schemars(with = "Option<Vec<String>>")]
    pub canisters: CanisterSelection,

    /// An optional list of the canisters to leave out of this environment.
    /// Applied after `canisters`, so it narrows either the whole project or an
    /// explicit selection. Empty when the manifest names none.
    #[schemars(rename = "exclude-canisters", with = "Option<Vec<CanisterExclusion>>")]
    pub exclude_canisters: Vec<CanisterExclusion>,

    /// Override the canister settings for this environment
    pub settings: Option<HashMap<String, ManifestSettings>>,

    /// Override init args for specific canisters in this environment
    pub init_args: Option<HashMap<String, ManifestInitArgs>>,
}

impl From<EnvironmentInner> for EnvironmentManifest {
    fn from(v: EnvironmentInner) -> Self {
        let EnvironmentInner {
            name,
            network,
            canisters,
            exclude_canisters,
            settings,
            init_args,
        } = v;

        // Network
        let network = network.unwrap_or(LOCAL.to_string());

        // Canisters
        let canisters = match canisters {
            // If the caller provided a list of canisters
            Some(cs) => match cs.is_empty() {
                // An empty list means explicitly "no canisters"
                true => CanisterSelection::None,

                // Non-empty list means targeting these specific canisters
                false => CanisterSelection::Named(cs),
            },

            // If no list was provided, assume all canisters are targeted
            None => CanisterSelection::Everything,
        };

        Self {
            name,
            network,
            canisters,
            exclude_canisters: exclude_canisters.unwrap_or_default(),

            // Keep as-is, setting overrides is optional
            settings,
            init_args,
        }
    }
}

impl<'de> Deserialize<'de> for EnvironmentManifest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let inner: EnvironmentInner = Deserialize::deserialize(d)?;
        Ok(inner.into())
    }
}

impl From<&EnvironmentManifest> for EnvironmentInner {
    fn from(env: &EnvironmentManifest) -> Self {
        let network = if env.network == LOCAL {
            None
        } else {
            Some(env.network.clone())
        };

        let canisters = match &env.canisters {
            CanisterSelection::Everything => None,
            CanisterSelection::Named(names) => Some(names.clone()),
            CanisterSelection::None => Some(vec![]),
        };

        let exclude_canisters = match env.exclude_canisters.is_empty() {
            true => None,
            false => Some(env.exclude_canisters.clone()),
        };

        EnvironmentInner {
            name: env.name.clone(),
            network,
            canisters,
            exclude_canisters,
            settings: env.settings.clone(),
            init_args: env.init_args.clone(),
        }
    }
}

impl Serialize for EnvironmentManifest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EnvironmentInner::from(self).serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        assert_eq!(
            serde_yaml::from_str::<EnvironmentManifest>(
                r#"
                name: my-environment
                "#
            )
            .expect("failed to deserialize EnvironmentManifest from yaml"),
            EnvironmentManifest {
                name: "my-environment".to_string(),
                network: "local".to_string(),
                canisters: CanisterSelection::Everything,
                exclude_canisters: vec![],
                settings: None,
                init_args: None,
            },
        );
    }

    #[test]
    fn exclude_canisters() {
        assert_eq!(
            serde_yaml::from_str::<EnvironmentManifest>(
                r#"
                name: my-environment
                canisters: [frontend, backend]
                exclude-canisters:
                  - backend
                  - services/crm:worker
                  - "services/crm:"
                  - services/crm:
                "#
            )
            .expect("failed to deserialize EnvironmentManifest from yaml")
            .exclude_canisters,
            vec![
                CanisterExclusion::Canister("backend".to_string()),
                CanisterExclusion::Canister("services/crm:worker".to_string()),
                // Quoted, and the unquoted spelling YAML reads as a mapping.
                CanisterExclusion::Subproject("services/crm".to_string()),
                CanisterExclusion::Subproject("services/crm".to_string()),
            ],
        );
    }

    #[test]
    fn exclude_canisters_round_trips() {
        let env = EnvironmentManifest {
            name: "my-environment".to_string(),
            network: "local".to_string(),
            canisters: CanisterSelection::Everything,
            exclude_canisters: vec![
                CanisterExclusion::Canister("backend".to_string()),
                CanisterExclusion::Subproject("services/crm".to_string()),
            ],
            settings: None,
            init_args: None,
        };
        let yaml = serde_yaml::to_string(&env).expect("failed to serialize EnvironmentManifest");
        assert!(
            yaml.contains("exclude-canisters:"),
            "expected a kebab-case key, got:\n{yaml}"
        );
        assert_eq!(
            serde_yaml::from_str::<EnvironmentManifest>(&yaml)
                .expect("failed to deserialize EnvironmentManifest from yaml"),
            env,
        );
    }

    #[test]
    fn exclusion_matches_canisters_and_subprojects() {
        let canister = CanisterExclusion::Canister("services/crm:worker".to_string());
        assert!(canister.matches("services/crm:worker"));
        assert!(!canister.matches("services/crm:backend"));
        assert!(!canister.matches("worker"));

        // A subproject covers its own canisters and those of the subprojects
        // nested beneath it, but not a sibling whose key merely starts the same.
        let subproject = CanisterExclusion::Subproject("services/crm".to_string());
        assert!(subproject.matches("services/crm:worker"));
        assert!(subproject.matches("services/crm/vendor/ledger:ledger"));
        assert!(!subproject.matches("services/crm-legacy:worker"));
        assert!(!subproject.matches("services:worker"));
        assert!(!subproject.matches("worker"));
    }

    #[test]
    fn exclusion_rejects_empty_entries() {
        assert!(matches!(
            "".parse::<CanisterExclusion>(),
            Err(ParseCanisterExclusionError::Empty),
        ));
        assert!(matches!(
            ":".parse::<CanisterExclusion>(),
            Err(ParseCanisterExclusionError::EmptySubproject),
        ));
    }
}
