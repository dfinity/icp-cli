use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

use crate::canister::{Settings, sync};

use super::{adapter, recipe::Recipe, serde_helpers::non_empty_vec};

/// Represents the manifest describing a single canister.
/// This struct is typically loaded from a `canister.yaml` file and defines
/// the canister's name and how it should be built into WebAssembly.
#[derive(Clone, Debug, PartialEq, JsonSchema)]
pub struct CanisterManifest {
    /// The unique name of the canister as defined in this manifest.
    pub name: String,

    /// The configuration specifying the various settings when creating the canister.
    #[serde(default)]
    #[schemars(with = "Option<Settings>")]
    pub settings: Settings,

    #[serde(flatten)]
    pub instructions: Instructions,
}

impl<'de> Deserialize<'de> for CanisterManifest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};
        use std::fmt;

        struct CanisterManifestVisitor;

        impl<'de> Visitor<'de> for CanisterManifestVisitor {
            type Value = CanisterManifest;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a canister manifest with a name, optional settings and either a recipe or build instructions")
            }

            // We're going to build the canister manifest manually
            // to be able to give good error messages
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut temp_map = serde_yaml::Mapping::new();
                while let Some((key, value)) =
                    map.next_entry::<serde_yaml::Value, serde_yaml::Value>()?
                {
                    temp_map.insert(key, value);
                }

                // All the keys to check
                let name_key = serde_yaml::Value::String("name".to_string());
                let settings_key = serde_yaml::Value::String("settings".to_string());
                let recipe_key = serde_yaml::Value::String("recipe".to_string());
                let build_key = serde_yaml::Value::String("build".to_string());
                let sync_key = serde_yaml::Value::String("sync".to_string());

                // Extract name (required)
                let name: String = temp_map
                    .remove(&name_key)
                    .ok_or_else(|| Error::custom("missing 'name' field"))?
                    .as_str()
                    .ok_or_else(|| Error::custom("'name' must be a string"))?
                    .to_string();

                // Extract settings (optional, with default)
                let settings: Settings =
                    if let Some(settings_value) = temp_map.remove(&settings_key) {
                        serde_yaml::from_value(settings_value).map_err(|e| {
                            Error::custom(format!(
                                "Failed to parse settings for canister `{name}`: {}",
                                e
                            ))
                        })?
                    } else {
                        Settings::default()
                    };

                //
                // Build out the instructions
                //
                let has_recipe = temp_map.contains_key(&recipe_key);
                let has_build = temp_map.contains_key(&build_key);
                let has_sync = temp_map.contains_key(&sync_key);

                match (has_recipe, has_build, has_sync) {
                    (true, true, _) => {
                        // Can't have a recipe and a build
                        Err(Error::custom(format!(
                            "Canister {name} cannot have both a `recipe` and a `build` section"
                        )))
                    }
                    (true, false, true) => {
                        // Can't have a recipe and a sync sections
                        Err(Error::custom(format!(
                            "Canister {name} cannot have both a `recipe` and a `sync` section"
                        )))
                    }
                    (false, false, _) => {
                        // We must have recipe or build
                        Err(Error::custom(format!(
                            "Canister {name} must have a `recipe` or a `build` section"
                        )))
                    }
                    (true, false, false) => {
                        // We have a a recipe
                        let recipe: Recipe = serde_yaml::from_value(
                            temp_map
                                .remove(&recipe_key)
                                .ok_or_else(|| Error::custom("recipe field not found"))?
                                .clone(),
                        )
                        .map_err(|e| {
                            Error::custom(format!("Canister {name} failed to parse recipe: {}", e))
                        })?;

                        if !temp_map.is_empty() {
                            return Err(Error::custom(format!(
                                "Unrecognized fields in canister `{name}`."
                            )));
                        }

                        Ok(CanisterManifest {
                            name,
                            settings,
                            instructions: Instructions::Recipe { recipe },
                        })
                    }
                    (false, true, _) => {
                        // We have a build section

                        // Try to deserialize as BuildSync variant
                        #[derive(Deserialize)]
                        #[serde(deny_unknown_fields)]
                        struct BuildSyncHelper {
                            build: BuildSteps,
                            sync: Option<sync::Steps>,
                        }

                        let helper: BuildSyncHelper = serde_yaml::from_value(
                            serde_yaml::Value::Mapping(temp_map),
                        )
                        .map_err(|e| {
                            Error::custom(format!(
                                "Canister {name} failed to parse build/sync instructions: {:#?}",
                                e
                            ))
                        })?;

                        Ok(CanisterManifest {
                            name,
                            settings,
                            instructions: Instructions::BuildSync {
                                build: helper.build,
                                sync: helper.sync,
                            },
                        })
                    }
                }
            }
        }

        d.deserialize_map(CanisterManifestVisitor)
    }
}

#[derive(Clone, Debug, PartialEq, JsonSchema, Deserialize)]
#[serde(untagged)]
pub enum Instructions {
    Recipe {
        recipe: Recipe,
    },

    BuildSync {
        /// The build configuration specifying how to compile the canister's source
        /// code into a WebAssembly module, including the adapter to use.
        build: BuildSteps,

        /// The configuration specifying how to sync the canister
        sync: Option<sync::Steps>,
    },
}

/// Identifies the type of adapter used to build the canister,
/// along with its configuration.
///
/// The adapter type is specified via the `type` field in the YAML file.
/// For example:
///
/// ```yaml
/// type: script
/// command: do_something.sh
/// ```
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum BuildStep {
    /// Represents a canister built using a custom script or command.
    /// This variant allows for flexible build processes defined by the user.
    Script(adapter::script::Adapter),

    /// Represents a pre-built canister.
    /// This variant allows for retrieving a canister WASM from various sources.
    #[serde(rename = "pre-built")]
    Prebuilt(adapter::prebuilt::Adapter),
}

impl fmt::Display for BuildStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                BuildStep::Script(v) => format!("(script)\n{v}"),
                BuildStep::Prebuilt(v) => format!("(pre-built)\n{v}"),
            }
        )
    }
}

/// Describes how the canister should be built into WebAssembly,
/// including the adapters and build steps responsible for the build.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct BuildSteps {
    #[serde(deserialize_with = "non_empty_vec")]
    pub steps: Vec<BuildStep>,
}

#[cfg(test)]
mod tests {
    use core::panic;
    use indoc::indoc;
    use std::collections::HashMap;

    use crate::manifest::{
        adapter::{
            assets,
            prebuilt::{self, RemoteSource, SourceField},
            script,
        },
        recipe::RecipeType,
    };

    use super::*;

    const CANNOT_HAVE_BOTH: &str =
        "Canister my-canister cannot have both a `recipe` and a `build` section";
    const ARRAY_NOT_EMPTY: &str = "Array must not be empty";

    /// Validates project yaml against the schema and deserializes it to a manifest
    fn validate_canister_yaml(s: &str) -> CanisterManifest {
        let schema = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../../docs/schemas/canister-yaml-schema.json"
        ))
        .expect("failed to deserialize canister.yaml schema");
        let canister_yaml = serde_yaml::from_str::<serde_json::Value>(s)
            .expect("failed to deserialize canister.yaml");

        // Build & reuse
        let validator = jsonschema::options()
            .build(&schema)
            .expect("failed to build jsonschema validator");

        // Iterate over errors
        for error in validator.iter_errors(&canister_yaml) {
            eprintln!("--------- Error ----------");
            eprintln!("{error:#?}");
            eprintln!("--------------------------");
        }

        assert!(validator.is_valid(&canister_yaml));

        serde_yaml::from_str::<CanisterManifest>(s)
            .expect("failed to deserialize CanisterManifest from yaml")
    }

    #[test]
    fn empty() {
        match serde_yaml::from_str::<CanisterManifest>(r#"name: my-canister"#) {
            // No Error
            Ok(_) => {
                panic!("an empty canister manifest should result in an error");
            }

            // Wrong Error
            Err(err) => {
                if !format!("{err}")
                    .starts_with("Canister my-canister must have a `recipe` or a `build` section")
                {
                    panic!("an empty canister manifest resulted in the wrong error: {err}");
                };
            }
        };
    }

    #[test]
    #[should_panic]
    fn canister_with_recipe() {
        // This should now fail because "unknown_type" is not a valid recipe type
        let _ = validate_canister_yaml(indoc! {r#"
            name: my-canister
            recipe:
              type: unknown_type
              configuration:
                field: value

        "#});
    }

    #[test]
    fn canister_with_build() {
        validate_canister_yaml(indoc! {r#"
            name: my-canister
            build:
              steps:
                - type: script
                  command: dosomething.sh
        "#});
    }

    #[test]
    fn invalid_manifest_mix_recipe_and_build() {
        match serde_yaml::from_str::<CanisterManifest>(indoc! {r#"
                name: my-canister
                recipe:
                  type: file://my-recipe
                build:
                  steps:
                    - type: pre-built
                      url: http://example.com/hello_world.wasm
                      sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
        "#})
        {
            Ok(_) => {
                panic!("You should not be able to have a recipe and build steps at the same time");
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains(CANNOT_HAVE_BOTH) {
                    panic!("expected '{CANNOT_HAVE_BOTH}' error but got: {err}");
                }
            }
        };
    }

    #[test]
    fn invalid_manifest_build_with_unrecognized_fields() {
        match serde_yaml::from_str::<CanisterManifest>(indoc! {r#"
                name: my-canister
                build:
                  steps:
                    - type: pre-built
                      url: http://example.com/hello_world.wasm
                invalid: invalid
        "#})
        {
            Ok(_) => {
                panic!("We don't allow unrecognized fields in a canister definition");
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains("Canister my-canister failed to parse build/sync") {
                    panic!(
                        "expected 'Canister my-canister failed to parse build/sync' error but got: {err}"
                    );
                }
            }
        };
    }

    #[test]
    fn invalid_manifest_recipe_with_unrecognized_fields() {
        match serde_yaml::from_str::<CanisterManifest>(indoc! {r#"
                name: my-canister
                recipe:
                  type: file://my-recipe
                invalid: invalid
        "#})
        {
            Ok(_) => {
                panic!("We don't allow unrecognized fields in a canister definition");
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains("Unrecognized fields in canister `my-canister`") {
                    panic!(
                        "expected 'Unrecognized fields in canister `my-canister`' error but got: {err}"
                    );
                }
            }
        };
    }

    #[test]
    fn invalid_manifest_mix_bad_recipe_and_build() {
        match serde_yaml::from_str::<CanisterManifest>(indoc! {r#"
                    name: my-canister
                    recipe:
                      type: INVALID
                    build:
                      steps:
                        - type: pre-built
                          url: http://example.com/hello_world.wasm
                          sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
            "#})
        {
            Ok(_) => {
                panic!("You should not be able to have a recipe and build steps at the same time");
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains(CANNOT_HAVE_BOTH) {
                    panic!("expected '{CANNOT_HAVE_BOTH}' error but got: {err}");
                }
            }
        };
    }

    #[test]
    fn invalid_manifest_mix_recipe_and_bad_build() {
        match serde_yaml::from_str::<CanisterManifest>(indoc! {r#"
                    name: my-canister
                    recipe:
                      type: file://template
                    build:
                      invalid: INVALID
            "#})
        {
            Ok(_) => {
                panic!("You should not be able to have a recipe and build steps at the same time");
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains(CANNOT_HAVE_BOTH) {
                    panic!("expected '{CANNOT_HAVE_BOTH}' error but got: {err}");
                }
            }
        };
    }

    #[test]
    fn recipe() {
        assert_eq!(
            validate_canister_yaml(indoc! {r#"
                    name: my-canister
                    recipe:
                      type: file://my-recipe
                "#}),
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
    }

    #[test]
    fn recipe_with_configuration() {
        assert_eq!(
            validate_canister_yaml(indoc! {r#"
                    name: my-canister
                    recipe:
                      type: http://my-recipe
                      configuration:
                        key-1: value-1
                        key-2: value-2
                "#}),
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
    }

    #[test]
    fn recipe_with_sha256() {
        assert_eq!(
            validate_canister_yaml(indoc! {r#"
                    name: my-canister
                    recipe:
                      type: "@dfinity/dummy"
                      sha256: 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08
                "#}),
            CanisterManifest {
                name: "my-canister".to_string(),
                settings: Settings::default(),
                instructions: Instructions::Recipe {
                    recipe: Recipe {
                        recipe_type: RecipeType::Registry {
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
    }

    #[test]
    fn recipe_with_settings() {
        assert_eq!(
            validate_canister_yaml(indoc! {r#"
                    name: my-canister
                    settings:
                      compute_allocation: 3
                      memory_allocation: 4294967296
                    recipe:
                      type: file://my-recipe
                "#}),
            CanisterManifest {
                name: "my-canister".to_string(),
                settings: Settings {
                    compute_allocation: Some(3),
                    memory_allocation: Some(4294967296),
                    ..Default::default()
                },
                instructions: Instructions::Recipe {
                    recipe: Recipe {
                        recipe_type: RecipeType::File("my-recipe".to_string()),
                        configuration: HashMap::new(),
                        sha256: None,
                    }
                },
            },
        );
    }

    #[test]
    fn build_steps() {
        assert_eq!(
            validate_canister_yaml(indoc! {r#"
                    name: my-canister
                    build:
                      steps:
                        - type: pre-built
                          url: http://example.com/hello_world.wasm
                          sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
                "#}),
            CanisterManifest {
                name: "my-canister".to_string(),
                settings: Settings::default(),
                instructions: Instructions::BuildSync {
                    build: BuildSteps {
                        steps: vec![BuildStep::Prebuilt(prebuilt::Adapter {
                            source: SourceField::Remote(RemoteSource {
                                url: "http://example.com/hello_world.wasm".to_string()
                            }),
                            sha256: Some(
                                "17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a"
                                    .to_string()
                            )
                        }),]
                    },
                    sync: None,
                },
            },
        );
    }

    #[test]
    fn empty_steps_is_not_allowed() {
        match serde_yaml::from_str::<CanisterManifest>(indoc! {r#"
                    name: my-canister
                    build:
                      steps: []
                    sync:
                      steps:
                        - type: assets
                          dir: dist
            "#})
        {
            Ok(_) => {
                panic!("You should not be able to have a recipe and build steps at the same time");
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains(ARRAY_NOT_EMPTY) {
                    panic!("expected '{ARRAY_NOT_EMPTY}' error but got: {err}");
                }
            }
        };
    }

    #[test]
    fn sync_steps() {
        assert_eq!(
            validate_canister_yaml(indoc! {r#"
                name: my-canister
                build:
                  steps:
                    - type: script
                      command: dosomething.sh
                sync:
                  steps:
                    - type: assets
                      dir: dist
            "#}),
            CanisterManifest {
                name: "my-canister".to_string(),
                settings: Settings::default(),
                instructions: Instructions::BuildSync {
                    build: BuildSteps {
                        steps: vec![BuildStep::Script(script::Adapter {
                            command: script::CommandField::Command("dosomething.sh".to_string()),
                        })]
                    },
                    sync: Some(sync::Steps {
                        steps: vec![sync::Step::Assets(assets::Adapter {
                            dir: assets::DirField::Dir("dist".to_string()),
                        })]
                    }),
                },
            },
        );
    }
}
