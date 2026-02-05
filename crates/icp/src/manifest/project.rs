use schemars::JsonSchema;
use serde::Deserialize;

use crate::manifest::{
    Item, canister::CanisterManifest, environment::EnvironmentManifest, network::NetworkManifest,
};

#[derive(Debug, PartialEq, JsonSchema, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    #[serde(default)]
    #[schemars(with = "Option<Vec<Item<CanisterManifest>>>")]
    pub canisters: Vec<Item<CanisterManifest>>,

    #[serde(default)]
    #[schemars(with = "Option<Vec<Item<NetworkManifest>>>")]
    pub networks: Vec<Item<NetworkManifest>>,

    #[serde(default)]
    #[schemars(with = "Option<Vec<Item<EnvironmentManifest>>>")]
    pub environments: Vec<Item<EnvironmentManifest>>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use indoc::indoc;

    use crate::{
        canister::Settings,
        manifest::{
            adapter::script,
            canister::{BuildStep, BuildSteps, Instructions},
            environment::CanisterSelection,
            network::{Managed, ManagedMode, Mode},
        },
    };

    use super::*;

    /// Validates project yaml against the schema and deserializes it to a manifest
    fn validate_project_yaml(s: &str) -> ProjectManifest {
        let schema = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../../docs/schemas/icp-yaml-schema.json"
        ))
        .expect("failed to deserialize project.yaml schema");
        let project_yaml = serde_yaml::from_str::<serde_json::Value>(s)
            .expect("failed to deserialize project.yaml");

        // Build & reuse
        let validator = jsonschema::options()
            .build(&schema)
            .expect("failed to build jsonschema validator");

        // Iterate over errors
        for error in validator.iter_errors(&project_yaml) {
            eprintln!("Error: {error:#?}");
        }

        assert!(validator.is_valid(&project_yaml));

        serde_yaml::from_str::<ProjectManifest>(s).expect("failed to deserialize project.yaml")
    }

    #[test]
    fn validate_manifest_against_schema() {
        let _ = validate_project_yaml(indoc! {r#"
                canisters:
                  - name: my-canister

                    build:
                      steps:
                        - type: pre-built
                          path: ../icp-pre-built/dist/hello_world.wasm
                          sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a

                    settings:
                      environment_variables:
                        var-1: value-1
                        var-2: value-2
                        var-3: value-3

            "#});
    }

    #[test]
    fn validate_canister_list_manifest() {
        let _ = validate_project_yaml(indoc! {r#"
                canisters:
                  - name: my-canister
                    build:
                      steps:
                        - type: pre-built
                          path: ../icp-pre-built/dist/hello_world.wasm
                          sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a

                    settings:
                      environment_variables:
                        var-1: value-1
                        var-2: value-2
                        var-3: value-3
                  - path/to/directory/

                environments:
                  - zoblamcouche

            "#});
    }

    #[test]
    fn empty() {
        assert_eq!(
            serde_yaml::from_str::<ProjectManifest>(r#""#).unwrap(),
            ProjectManifest {
                canisters: vec![],
                networks: vec![],
                environments: vec![],
            },
        );
    }

    #[test]
    fn canister() {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                    canisters:
                      - name: my-canister
                        build:
                          steps:
                            - type: script
                              command: dosomething.sh
                "#}),
            ProjectManifest {
                canisters: vec![Item::Manifest(CanisterManifest {
                    name: "my-canister".to_string(),
                    settings: Settings::default(),
                    init_args: None,
                    instructions: Instructions::BuildSync {
                        build: BuildSteps {
                            steps: vec![BuildStep::Script(script::Adapter {
                                command: script::CommandField::Command(
                                    "dosomething.sh".to_string()
                                )
                            })]
                        },
                        sync: None,
                    },
                })],
                networks: vec![],
                environments: vec![],
            },
        );
    }

    #[test]
    fn project_with_invalid_canister_should_fail() {
        // This canister is invalid because
        match serde_yaml::from_str::<ProjectManifest>(indoc! {r#"
                canisters:
                  - name: my-canister
                    build:
                      steps: []
            "#})
        {
            Ok(_) => {
                panic!("A project manifest with an invalid canister manifest should be invalid");
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains(
                    "Canister my-canister failed to parse build/sync instructions",
                ) {
                    panic!(
                        "expected 'Canister my-canister failed to parse build/sync instructions' error but got: {err}"
                    );
                }
            }
        };
    }

    #[test]
    fn project_with_invalid_recipe_type_should_fail() {
        // Test that errors from nested deserialization are properly propagated
        // through the custom Item<T> deserializer
        match serde_yaml::from_str::<ProjectManifest>(indoc! {r#"
                canisters:
                  - name: my-canister
                    recipe:
                      type: blabla
            "#})
        {
            Ok(_) => {
                panic!("A project manifest with an invalid canister manifest should be invalid");
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains("Invalid recipe type: `blabla`") {
                    panic!("expected 'Invalid recipe type: `blabla`' error but got: {err}");
                }
            }
        };
    }

    #[test]
    fn canisters_in_list() {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                    canisters:
                      - name: my-canister
                        build:
                          steps:
                            - type: script
                              command: dosomething.sh
                "#}),
            ProjectManifest {
                canisters: vec![Item::Manifest(CanisterManifest {
                    name: "my-canister".to_string(),
                    settings: Settings::default(),
                    init_args: None,
                    instructions: Instructions::BuildSync {
                        build: BuildSteps {
                            steps: vec![BuildStep::Script(script::Adapter {
                                command: script::CommandField::Command(
                                    "dosomething.sh".to_string()
                                )
                            })]
                        },
                        sync: None,
                    },
                })],
                networks: vec![],
                environments: vec![],
            },
        );
    }

    #[test]
    fn canisters_mixed() {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                    canisters:
                      - name: my-canister
                        build:
                          steps:
                            - type: script
                              command: dosomething.sh
                      - canisters/*
                "#}),
            ProjectManifest {
                canisters: vec![
                    Item::Manifest(CanisterManifest {
                        name: "my-canister".to_string(),
                        settings: Settings::default(),
                        init_args: None,
                        instructions: crate::manifest::canister::Instructions::BuildSync {
                            build: BuildSteps {
                                steps: vec![BuildStep::Script(script::Adapter {
                                    command: script::CommandField::Command(
                                        "dosomething.sh".to_string()
                                    )
                                })]
                            },
                            sync: None,
                        },
                    }),
                    Item::Path("canisters/*".to_string())
                ],
                networks: vec![],
                environments: vec![],
            },
        );
    }

    #[test]
    fn networks() {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                    networks:
                      - name: my-network
                        mode: managed
                "#}),
            ProjectManifest {
                canisters: vec![],
                networks: vec![Item::Manifest(NetworkManifest {
                    name: "my-network".to_string(),
                    configuration: Mode::Managed(Managed {
                        mode: Box::new(ManagedMode::Launcher {
                            gateway: None,
                            artificial_delay_ms: None,
                            ii: None,
                            nns: None,
                            subnets: None
                        }),
                    }),
                })],
                environments: vec![],
            },
        );
    }

    #[test]
    fn environment() {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                    environments:
                      - name: my-environment
                        network: my-network
                        canisters: [my-canister]
                "#}),
            ProjectManifest {
                canisters: vec![],
                networks: vec![],
                environments: vec![Item::Manifest(EnvironmentManifest {
                    name: "my-environment".to_string(),
                    network: "my-network".to_string(),
                    canisters: CanisterSelection::Named(vec!["my-canister".to_string()]),
                    settings: None,
                    init_args: None,
                })],
            },
        );
    }

    #[test]
    fn environment_in_list() {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                    environments:
                      - name: my-environment
                        network: my-network
                        canisters: [my-canister]
                "#}),
            ProjectManifest {
                canisters: vec![],
                networks: vec![],
                environments: vec![Item::Manifest(EnvironmentManifest {
                    name: "my-environment".to_string(),
                    network: "my-network".to_string(),
                    canisters: CanisterSelection::Named(vec!["my-canister".to_string()]),
                    settings: None,
                    init_args: None,
                })],
            },
        );
    }

    #[test]
    fn environment_canister_selection() {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                    environments:
                      - name: environment-1
                        canisters: []
                      - name: environment-2
                        canisters: [my-canister]
                      - name: environment-3
                "#}),
            ProjectManifest {
                canisters: vec![],
                networks: vec![],
                environments: vec![
                    Item::Manifest(EnvironmentManifest {
                        name: "environment-1".to_string(),
                        network: "local".to_string(),
                        canisters: CanisterSelection::None,
                        settings: None,
                        init_args: None,
                    }),
                    Item::Manifest(EnvironmentManifest {
                        name: "environment-2".to_string(),
                        network: "local".to_string(),
                        canisters: CanisterSelection::Named(vec!["my-canister".to_string()]),
                        settings: None,
                        init_args: None,
                    }),
                    Item::Manifest(EnvironmentManifest {
                        name: "environment-3".to_string(),
                        network: "local".to_string(),
                        canisters: CanisterSelection::Everything,
                        settings: None,
                        init_args: None,
                    }),
                ],
            },
        );
    }

    #[test]
    fn environment_settings() {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                    environments:
                      - name: my-environment
                        settings:
                          canister-1:
                            compute_allocation: 1
                          canister-2:
                            compute_allocation: 2
                "#}),
            ProjectManifest {
                canisters: vec![],
                networks: vec![],
                environments: vec![Item::Manifest(EnvironmentManifest {
                    name: "my-environment".to_string(),
                    network: "local".to_string(),
                    canisters: CanisterSelection::Everything,
                    settings: Some(HashMap::from([
                        (
                            "canister-1".to_string(),
                            Settings {
                                compute_allocation: Some(1),
                                ..Default::default()
                            }
                        ),
                        (
                            "canister-2".to_string(),
                            Settings {
                                compute_allocation: Some(2),
                                ..Default::default()
                            }
                        )
                    ])),
                    init_args: None,
                })],
            },
        );
    }

    #[test]
    fn environment_init_args() {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                    environments:
                      - name: my-environment
                        init_args:
                          canister-1: "(42)"
                          canister-2: "4449444c0000"
                "#}),
            ProjectManifest {
                canisters: vec![],
                networks: vec![],
                environments: vec![Item::Manifest(EnvironmentManifest {
                    name: "my-environment".to_string(),
                    network: "local".to_string(),
                    canisters: CanisterSelection::Everything,
                    settings: None,
                    init_args: Some(HashMap::from([
                        ("canister-1".to_string(), "(42)".to_string()),
                        ("canister-2".to_string(), "4449444c0000".to_string()),
                    ])),
                })],
            },
        );
    }

    #[test]
    fn invalid() {
        if serde_yaml::from_str::<ProjectManifest>(r#"invalid-content"#).is_ok() {
            panic!("expected invalid manifest to fail deserializeation");
        }
    }
}
