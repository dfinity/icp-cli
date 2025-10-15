use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

use crate::{
    canister::{Settings, build, sync},
    manifest::recipe::Recipe,
};

#[derive(Clone, Debug, PartialEq, JsonSchema)]
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

impl<'de> Deserialize<'de> for Instructions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{Error, MapAccess, Visitor};
        use std::fmt;

        struct InstructionsVisitor;

        impl<'de> Visitor<'de> for InstructionsVisitor {
            type Value = Instructions;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("either a recipe or build/sync instructions")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                // Collect all key-value pairs into a temporary map
                let mut temp_map = serde_yaml::Mapping::new();
                while let Some((key, value)) = map.next_entry::<serde_yaml::Value, serde_yaml::Value>()? {
                    temp_map.insert(key, value);
                }

                let recipe_key = serde_yaml::Value::String("recipe".to_string());
                let build_key = serde_yaml::Value::String("build".to_string());
                
                let has_recipe = temp_map.contains_key(&recipe_key);
                let has_build = temp_map.contains_key(&build_key);

                // We don't allow both a recipe and a build step
                if has_recipe && has_build {
                    return Err(Error::custom("Canister manifest cannot have both a recipe and build instructions"));
                }

                if has_recipe {

                    // Try to deserialize as Recipe variant - propagate any errors
                    let recipe: Recipe = serde_yaml::from_value(
                        temp_map.get(&recipe_key)
                            .ok_or_else(|| Error::custom("recipe field not found"))?
                            .clone()
                    ).map_err(|e| Error::custom(format!("Failed to parse recipe: {}", e)))?;
                    Ok(Instructions::Recipe { recipe })

                } else if has_build {

                    // Try to deserialize as BuildSync variant
                    #[derive(Deserialize)]
                    struct BuildSyncHelper {
                        build: build::Steps,
                        #[serde(default)]
                        sync: sync::Steps,
                    }

                    let helper: BuildSyncHelper = serde_yaml::from_value(serde_yaml::Value::Mapping(temp_map))
                        .map_err(|e| Error::custom(format!("Failed to parse build/sync instructions: {}", e)))?;
                    
                    Ok(Instructions::BuildSync {
                        build: helper.build,
                        sync: helper.sync,
                    })

                } else {
                    // Neither recipe nor build present, we must have one of the two
                    Err(Error::custom("missing recipe or build instructions"))
                }
            }
        }

        deserializer.deserialize_map(InstructionsVisitor)
    }
}

/// Represents the manifest describing a single canister.
/// This struct is typically loaded from a `canister.yaml` file and defines
/// the canister's name and how it should be built into WebAssembly.
#[derive(Clone, Debug, PartialEq, JsonSchema)]
pub struct CanisterManifest {
    /// The unique name of the canister as defined in this manifest.
    pub name: String,

    /// The configuration specifying the various settings when
    /// creating the canister.
    pub settings: Settings,

    #[serde(flatten)]
    pub instructions: Instructions,
}

impl<'de> Deserialize<'de> for CanisterManifest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};
        use std::fmt;

        struct CanisterInnerVisitor;

        impl<'de> Visitor<'de> for CanisterInnerVisitor {
            type Value = CanisterManifest;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a canister manifest with name and optional settings/instructions")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut temp_map = serde_yaml::Mapping::new();
                while let Some((key, value)) = map.next_entry::<serde_yaml::Value, serde_yaml::Value>()? {
                    temp_map.insert(key, value);
                }

                let name_key = serde_yaml::Value::String("name".to_string());
                let settings_key = serde_yaml::Value::String("settings".to_string());

                // Extract name (required)
                let name: String = temp_map
                    .remove(&name_key)
                    .ok_or_else(|| Error::custom("missing 'name' field"))?
                    .as_str()
                    .ok_or_else(|| Error::custom("'name' must be a string"))?
                    .to_string();

                // Extract settings (optional, with default)
                let settings: Settings = if let Some(settings_value) = temp_map.remove(&settings_key) {
                    serde_yaml::from_value(settings_value)
                        .map_err(|e| Error::custom(format!("Failed to parse settings for canister `{name}`: {}", e)))?
                } else {
                    Settings::default()
                };

                if temp_map.is_empty() {
                    return Err(Error::custom(format!("Canister {name} is missing a `recipe` or a `build` section.")));
                }
                // Try to parse instructions from remaining fields
                let instructions =  serde_yaml::from_value(serde_yaml::Value::Mapping(temp_map))
                        .map_err(|e| Error::custom(format!("Failed to parse instructions for canister `{name}`: {}", e)))?;

                Ok(CanisterManifest {
                    name,
                    settings,
                    instructions,
                })
            }
        }

        d.deserialize_map(CanisterInnerVisitor)
    }
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
        match serde_yaml::from_str::<CanisterManifest>(r#"name: my-canister"#) {
            // No Error
            Ok(_) => {
                return Err(anyhow!(
                    "an empty canister manifest should result in an error"
                ));
            }

            // Wrong Error
            Err(err) => {
                if !format!("{err}").starts_with("Canister my-canister is missing a `recipe` or a `build` section") {
                    return Err(anyhow!(
                        "an empty canister manifest resulted in the wrong error: {err}"
                    ));
                };
            }
        };

        Ok(())
    }

    #[test]
    fn invalid_recipe_bad_type() -> Result<(), Error> {
        // This should now fail because "unknown_type" is not a valid recipe type
        match serde_yaml::from_str::<CanisterManifest>(
            r#"
            name: my-canister
            recipe:
              type: unknown_type
              configuration:
                field: value

            "#
        ) {
            Ok(_) => {
                return Err(anyhow!(
                    "An invalid recipe type should result in an error"
                ));
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains("Invalid recipe type") {
                    return Err(anyhow!(
                        "expected 'Invalid recipe type' error but got: {err}"
                    ));
                }
            }
        }

        Ok(())
    }

    #[test]
    fn invalid_manifest_mix_recipe_and_build() -> Result<(), Error> {
        match serde_yaml::from_str::<CanisterManifest>(r#"
                name: my-canister
                recipe:
                  type: file://my-recipe
                build:
                  steps:
                    - type: pre-built
                      url: http://example.com/hello_world.wasm
                      sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
        "#) {
            Ok(_) => {
                return Err(anyhow!(
                    "You should not be able to have a recipe and build steps at the same time"
                ));
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains("Canister manifest cannot have both") {
                    return Err(anyhow!(
                        "expected 'Canister manifest cannot have both' error but got: {err}"
                    ));
                }
            }
        };

        Ok(())
    }

    #[test]
    fn invalid_manifest_mix_bad_recipe_and_build() -> Result<(), Error> {
        match serde_yaml::from_str::<CanisterManifest>(r#"
                name: my-canister
                recipe:
                  type: INVALID
                build:
                  steps:
                    - type: pre-built
                      url: http://example.com/hello_world.wasm
                      sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
        "#) {
            Ok(_) => {
                return Err(anyhow!(
                    "You should not be able to have a recipe and build steps at the same time"
                ));
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains("Canister manifest cannot have both") {
                    return Err(anyhow!(
                        "expected 'Canister manifest cannot have both' error but got: {err}"
                    ));
                }
            }
        };

        Ok(())
    }

    #[test]
    fn invalid_manifest_mix_recipe_and_bad_build() -> Result<(), Error> {
        match serde_yaml::from_str::<CanisterManifest>(r#"
                name: my-canister
                recipe:
                  type: file://template
                build:
                  invalid: INVALID
        "#) {
            Ok(_) => {
                return Err(anyhow!(
                    "You should not be able to have a recipe and build steps at the same time"
                ));
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains("Canister manifest cannot have both") {
                    return Err(anyhow!(
                        "expected 'Canister manifest cannot have both' error but got: {err}"
                    ));
                }
            }
        };

        Ok(())
    }

    #[test]
    fn recipe() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<CanisterManifest>(
                r#"
                name: my-canister
                recipe:
                  type: file://my-recipe
                "#
            )?,
            CanisterManifest {
                name: "my-canister".to_string(),
                settings: Settings::default(),
                instructions: Instructions::Recipe {
                    recipe: Recipe {
                        recipe_type: RecipeType::File("my-recipe".to_string()),
                        configuration: HashMap::new(),
                        sha256: None,
                    }
                },
            },
        );

        Ok(())
    }

    #[test]
    fn recipe_with_configuration() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<CanisterManifest>(
                r#"
                name: my-canister
                recipe:
                  type: http://my-recipe
                  configuration:
                    key-1: value-1
                    key-2: value-2
                "#
            )?,
            CanisterManifest {
                name: "my-canister".to_string(),
                settings: Settings::default(),
                instructions: Instructions::Recipe {
                    recipe: Recipe {
                        recipe_type: RecipeType::Url("http://my-recipe".to_string()),
                        configuration: HashMap::from([
                            ("key-1".to_string(), "value-1".into()),
                            ("key-2".to_string(), "value-2".into())
                        ]),
                        sha256: None,
                    }
                },
            },
        );

        Ok(())
    }

    #[test]
    fn recipe_with_sha256() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<CanisterManifest>(
                r#"
                name: my-canister
                recipe:
                  type: "@dfinity/dummy"
                  sha256: 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08
                "#
            )?,
            CanisterManifest {
                name: "my-canister".to_string(),
                settings: Settings::default(),
                instructions: Instructions::Recipe {
                    recipe: Recipe {
                        recipe_type: RecipeType::Registry{
                            name: "dfinity".to_string(),
                            recipe: "dummy".to_string(),
                            version: "latest".to_string(),
                        },
                        configuration: HashMap::new(),
                        sha256: Some(
                            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
                                .to_string()
                        ),
                    }
                },
            },
        );

        Ok(())
    }

    #[test]
    fn build_steps() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<CanisterManifest>(
                r#"
                name: my-canister
                build:
                  steps:
                    - type: pre-built
                      url: http://example.com/hello_world.wasm
                      sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
                "#
            )?,
            CanisterManifest {
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
            serde_yaml::from_str::<CanisterManifest>(
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
            CanisterManifest {
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
