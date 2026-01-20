use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

use crate::{canister::Settings, prelude::LOCAL};

#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
pub struct EnvironmentInner {
    pub name: String,
    pub network: Option<String>,
    pub canisters: Option<Vec<String>>,
    pub settings: Option<HashMap<String, Settings>>,
    pub init_args: Option<HashMap<String, String>>,
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

    /// Override the canister settings for this environment
    pub settings: Option<HashMap<String, Settings>>,

    /// Override init args for specific canisters in this environment
    pub init_args: Option<HashMap<String, String>>,
}

impl From<EnvironmentInner> for EnvironmentManifest {
    fn from(v: EnvironmentInner) -> Self {
        let EnvironmentInner {
            name,
            network,
            canisters,
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
                settings: None,
                init_args: None,
            },
        );
    }
}
