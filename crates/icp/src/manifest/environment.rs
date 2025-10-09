use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

use crate::canister::Settings;

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct EnvironmentInner {
    pub name: String,
    pub network: Option<String>,
    pub canisters: Option<Vec<String>>,
    pub settings: Option<HashMap<String, Settings>>,
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
    // environment name
    pub name: String,

    // target network for canister deployment
    pub network: String,

    // canisters the environment should contain
    pub canisters: CanisterSelection,

    // canister settings overrides
    pub settings: Option<HashMap<String, Settings>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("Overriding the local environment is not supported.")]
    OverrideLocal,

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

impl TryFrom<EnvironmentInner> for EnvironmentManifest {
    type Error = ParseError;

    fn try_from(v: EnvironmentInner) -> Result<Self, Self::Error> {
        let EnvironmentInner {
            name,
            network,
            canisters,
            settings,
        } = v;

        // Name
        if name == "local" {
            return Err(ParseError::OverrideLocal);
        }

        // Network
        let network = network.unwrap_or("local".to_string());

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

        Ok(Self {
            name,
            network,
            canisters,

            // Keep as-is, setting overrides is optional
            settings,
        })
    }
}

impl<'de> Deserialize<'de> for EnvironmentManifest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let inner: EnvironmentInner = Deserialize::deserialize(d)?;
        inner.try_into().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use anyhow::{Error, anyhow};

    use super::*;

    #[test]
    fn empty() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<EnvironmentManifest>(
                r#"
                name: my-environment
                "#
            )?,
            EnvironmentManifest {
                name: "my-environment".to_string(),
                network: "local".to_string(),
                canisters: CanisterSelection::Everything,
                settings: None,
            },
        );

        Ok(())
    }

    #[test]
    fn override_local() -> Result<(), Error> {
        match serde_yaml::from_str::<EnvironmentManifest>(r#"name: local"#) {
            // No Error
            Ok(_) => {
                return Err(anyhow!(
                    "an environment named local should result in an error"
                ));
            }

            // Wrong Error
            Err(err) => {
                if !format!("{err}").starts_with("Overriding the local environment") {
                    return Err(anyhow!(
                        "an environment named local resulted in the wrong error: {err}"
                    ));
                };
            }
        };

        Ok(())
    }
}
