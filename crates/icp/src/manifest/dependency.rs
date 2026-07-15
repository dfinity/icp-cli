use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use super::environment::CanisterSelection;

/// Raw form of a dependency entry as written in `icp.yaml`.
///
/// `canisters` follows the same convention as an environment's canister
/// selection: omitted means "all", an empty list means "none".
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DependencyInner {
    pub name: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canisters: Option<Vec<String>>,
}

/// Declares a dependency on another `icp` project vendored into this one
/// (typically as a git submodule).
///
/// Running `icp deploy` deploys the dependency's canisters into the parent's
/// environment and injects the *selected* dependency canisters' IDs into the
/// parent's canisters as `PUBLIC_CANISTER_ID:<name>:<canister>` environment
/// variables.
#[derive(Clone, Debug, PartialEq, JsonSchema)]
pub struct DependencyManifest {
    /// Local alias for the dependency project. Namespaces the dependency's
    /// canister IDs when they are exposed to the parent's canisters as
    /// `PUBLIC_CANISTER_ID:<name>:<canister>` environment variables. Must be
    /// unique among dependencies, must not collide with a local canister name,
    /// and must not contain `:`.
    pub name: String,

    /// Path to the directory containing the dependency's `icp.yaml`, resolved
    /// relative to this manifest's project directory.
    pub path: String,

    /// Which of the dependency's canisters to **expose** to the parent as
    /// `PUBLIC_CANISTER_ID` environment variables. Defaults to all. This is an
    /// exposure filter only: the whole dependency is always deployed, because a
    /// dependency's canisters may call each other.
    #[schemars(with = "Option<Vec<String>>")]
    pub canisters: CanisterSelection,
}

impl From<DependencyInner> for DependencyManifest {
    fn from(v: DependencyInner) -> Self {
        let DependencyInner {
            name,
            path,
            canisters,
        } = v;

        let canisters = match canisters {
            Some(cs) => match cs.is_empty() {
                true => CanisterSelection::None,
                false => CanisterSelection::Named(cs),
            },
            None => CanisterSelection::Everything,
        };

        Self {
            name,
            path,
            canisters,
        }
    }
}

impl<'de> Deserialize<'de> for DependencyManifest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let inner: DependencyInner = Deserialize::deserialize(d)?;
        Ok(inner.into())
    }
}

impl From<&DependencyManifest> for DependencyInner {
    fn from(dep: &DependencyManifest) -> Self {
        let canisters = match &dep.canisters {
            CanisterSelection::Everything => None,
            CanisterSelection::Named(names) => Some(names.clone()),
            CanisterSelection::None => Some(vec![]),
        };

        DependencyInner {
            name: dep.name.clone(),
            path: dep.path.clone(),
            canisters,
        }
    }
}

impl Serialize for DependencyManifest {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        DependencyInner::from(self).serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expose_all_when_canisters_omitted() {
        assert_eq!(
            serde_yaml::from_str::<DependencyManifest>(
                r#"
                name: openemail
                path: ./vendor/openemail
                "#
            )
            .expect("failed to deserialize DependencyManifest"),
            DependencyManifest {
                name: "openemail".to_string(),
                path: "./vendor/openemail".to_string(),
                canisters: CanisterSelection::Everything,
            },
        );
    }

    #[test]
    fn named_subset() {
        assert_eq!(
            serde_yaml::from_str::<DependencyManifest>(
                r#"
                name: openemail
                path: ./vendor/openemail
                canisters: [backend]
                "#
            )
            .expect("failed to deserialize DependencyManifest"),
            DependencyManifest {
                name: "openemail".to_string(),
                path: "./vendor/openemail".to_string(),
                canisters: CanisterSelection::Named(vec!["backend".to_string()]),
            },
        );
    }

    #[test]
    fn empty_list_exposes_none() {
        assert_eq!(
            serde_yaml::from_str::<DependencyManifest>(
                r#"
                name: openemail
                path: ./vendor/openemail
                canisters: []
                "#
            )
            .expect("failed to deserialize DependencyManifest")
            .canisters,
            CanisterSelection::None,
        );
    }

    #[test]
    fn unknown_field_rejected() {
        assert!(
            serde_yaml::from_str::<DependencyManifest>(
                r#"
                name: openemail
                path: ./vendor/openemail
                bogus: true
                "#
            )
            .is_err()
        );
    }
}
