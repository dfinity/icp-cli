use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

use crate::{
    canister::{Settings, build, sync},
    manifest::recipe::Recipe,
};

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum Instructions {
    Recipe {
        recipe: Recipe,
    },

    BuildSync {
        /// The build configuration specifying how to compile the canister's source
        /// code into a WebAssembly module, including the adapter to use.
        build: build::Steps,

        /// The configuration specifying how to sync the canister
        #[serde(default)]
        sync: sync::Steps,
    },
}

/// Represents the manifest describing a single canister.
/// This struct is typically loaded from a `canister.yaml` file and defines
/// the canister's name and how it should be built into WebAssembly.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct CanisterInner {
    /// The unique name of the canister as defined in this manifest.
    pub name: String,

    /// The configuration specifying the various settings when
    /// creating the canister.
    #[serde(default)]
    pub settings: Settings,

    #[serde(flatten)]
    pub instructions: Option<Instructions>,
}

/// Represents the manifest describing a single canister.
/// This struct is typically loaded from a `canister.yaml` file and defines
/// the canister's name and how it should be built into WebAssembly.
#[derive(Clone, Debug, PartialEq, JsonSchema)]
pub struct Canister {
    /// The unique name of the canister as defined in this manifest.
    pub name: String,

    /// The configuration specifying the various settings when
    /// creating the canister.
    pub settings: Settings,

    pub instructions: Instructions,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error(
        "Please provide instructions for building your canister in the form of a recipe or build/sync steps."
    )]
    MissingInstructions,

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

impl TryFrom<CanisterInner> for Canister {
    type Error = ParseError;

    fn try_from(v: CanisterInner) -> Result<Self, Self::Error> {
        let CanisterInner {
            name,
            settings,
            instructions,
        } = v;

        // Instructions
        let instructions = instructions.ok_or(ParseError::MissingInstructions)?;

        Ok(Canister {
            name,
            settings,
            instructions,
        })
    }
}

impl<'de> Deserialize<'de> for Canister {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let inner: CanisterInner = Deserialize::deserialize(d)?;
        inner.try_into().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use anyhow::{Error, anyhow};

    use crate::manifest::{
        adapter::{
            assets,
            prebuilt::{self, RemoteSource, SourceField},
        },
        recipe::RecipeType,
    };

    use super::*;

    #[test]
    fn empty() -> Result<(), Error> {
        match serde_yaml::from_str::<Canister>(r#"name: my-canister"#) {
            // No Error
            Ok(_) => {
                return Err(anyhow!(
                    "an empty canister manifest should result in an error"
                ));
            }

            // Wrong Error
            Err(err) => {
                if !format!("{err}").starts_with("Please provide instructions") {
                    return Err(anyhow!(
                        "an empty canister manifest resulted in the wrong error: {err}"
                    ));
                };
            }
        };

        Ok(())
    }

    #[test]
    fn recipe() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Canister>(
                r#"
                name: my-canister
                recipe:
                  type: my-recipe
                "#
            )?,
            Canister {
                name: "my-canister".to_string(),
                settings: Settings::default(),
                instructions: Instructions::Recipe {
                    recipe: Recipe {
                        recipe_type: RecipeType::Unknown("my-recipe".to_string()),
                        configuration: HashMap::new()
                    }
                },
            },
        );

        Ok(())
    }

    #[test]
    fn recipe_with_configuration() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Canister>(
                r#"
                name: my-canister
                recipe:
                  type: my-recipe
                  configuration:
                    key-1: value-1
                    key-2: value-2
                "#
            )?,
            Canister {
                name: "my-canister".to_string(),
                settings: Settings::default(),
                instructions: Instructions::Recipe {
                    recipe: Recipe {
                        recipe_type: RecipeType::Unknown("my-recipe".to_string()),
                        configuration: HashMap::from([
                            ("key-1".to_string(), "value-1".into()),
                            ("key-2".to_string(), "value-2".into())
                        ])
                    }
                },
            },
        );

        Ok(())
    }

    #[test]
    fn build_steps() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Canister>(
                r#"
                name: my-canister
                build:
                  steps:
                    - type: pre-built
                      url: http://example.com/hello_world.wasm
                      sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
                "#
            )?,
            Canister {
                name: "my-canister".to_string(),
                settings: Settings::default(),
                instructions: Instructions::BuildSync {
                    build: build::Steps {
                        steps: vec![build::Step::Prebuilt(prebuilt::Adapter {
                            source: SourceField::Remote(RemoteSource {
                                url: "http://example.com/hello_world.wasm".to_string()
                            }),
                            sha256: Some(
                                "17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a"
                                    .to_string()
                            )
                        }),]
                    },
                    sync: sync::Steps { steps: vec![] },
                },
            },
        );

        Ok(())
    }

    #[test]
    fn sync_steps() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Canister>(
                r#"
                name: my-canister
                build:
                  steps: []
                sync:
                  steps:
                    - type: assets
                      dir: dist
                "#
            )?,
            Canister {
                name: "my-canister".to_string(),
                settings: Settings::default(),
                instructions: Instructions::BuildSync {
                    build: build::Steps { steps: vec![] },
                    sync: sync::Steps {
                        steps: vec![sync::Step::Assets(assets::Adapter {
                            dir: assets::DirField::Dir("dist".to_string()),
                        })]
                    },
                },
            },
        );

        Ok(())
    }
}
