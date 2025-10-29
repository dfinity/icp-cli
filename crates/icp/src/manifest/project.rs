use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

use crate::manifest::{
    Item, canister::CanisterManifest, environment::EnvironmentManifest, network::NetworkManifest,
};

#[derive(Debug, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    #[serde(flatten)]
    #[schemars(with = "Option<Canisters>")]
    pub canisters: Canisters,

    #[serde(default)]
    #[schemars(with = "Option<Vec<Item<NetworkManifest>>>")]
    pub networks: Vec<Item<NetworkManifest>>,

    #[serde(default)]
    #[schemars(with = "Option<Vec<Item<EnvironmentManifest>>>")]
    pub environments: Vec<Item<EnvironmentManifest>>,
}

#[derive(Clone, Debug, PartialEq, JsonSchema, Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)]
pub enum Canisters {
    Canister(CanisterManifest),
    Canisters(Vec<Item<CanisterManifest>>),
}

impl From<Canisters> for Vec<Item<CanisterManifest>> {
    fn from(value: Canisters) -> Self {
        match value {
            Canisters::Canister(v) => vec![Item::Manifest(v)],
            Canisters::Canisters(items) => items,
        }
    }
}

#[derive(Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Networks {
    Network(NetworkManifest),
    Networks(Vec<NetworkManifest>),
}

#[derive(Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Environments {
    Environment(EnvironmentManifest),
    Environments(Vec<EnvironmentManifest>),
}


impl<'de> Deserialize<'de> for ProjectManifest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};
        use std::fmt;

        struct ProjectManifestVisitor;

        impl<'de> Visitor<'de> for ProjectManifestVisitor {
            type Value = ProjectManifest;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("A project manifest, with canister, network and environment definitions")
            }

            // We're going to build the Project manifest manually
            // to be able to give good error messages
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut top_map = serde_yaml::Mapping::new();
                while let Some((key, value)) =
                    map.next_entry::<serde_yaml::Value, serde_yaml::Value>()?
                {
                    top_map.insert(key, value);
                }

                // Do the canisters first
                let canister_key = serde_yaml::Value::String("canister".to_string());
                let canisters_key = serde_yaml::Value::String("canisters".to_string());

                let has_canister_key = top_map.contains_key(&canister_key);
                let has_canisters_key = top_map.contains_key(&canisters_key);

                let canisters = match (has_canister_key, has_canisters_key) {
                    (true, true) => {
                        return Err(Error::custom("Can't have both `canister` and `canisters` definition"));
                    },
                    (true, false) => {
                        // single canister definition
                        let m : CanisterManifest = serde_yaml::from_value(
                            top_map.remove(&canister_key)
                                .ok_or_else(|| Error::custom("`canister` field not found"))?
                                .clone()
                        ).map_err( |e|
                            Error::custom(format!("Unable to parse canister manifest: {e}"))
                        )?;

                        Canisters::Canisters(vec![Item::Manifest::<CanisterManifest>(m)])
                    },
                    (false, true) => {
                        // Multi canister definition
                        if let serde_yaml::Value::Sequence(seq) = top_map
                            .remove(&canisters_key)
                            .ok_or_else(|| Error::custom("`canisters` key does not exist"))? {

                            if seq.is_empty() {
                                return Err(Error::custom("`canisters` cannot be empty"))
                            }

                            let mut canisters: Vec<Item<CanisterManifest>> =
                                Vec::with_capacity(seq.len());

                            for v in seq {
                                let item: Item<CanisterManifest> = match v {
                                    serde_yaml::Value::String(s) => Item::Path(s),
                                    serde_yaml::Value::Mapping(mapping) => {
                                        let canister_manifest: CanisterManifest =
                                            serde_yaml::from_value(mapping.into()).map_err(
                                                |e| {
                                                    Error::custom(format!(
                                                        "Failed to load canister manifest: {}",
                                                        e
                                                    ))
                                                },
                                            )?;
                                        Item::Manifest(canister_manifest)
                                    }
                                    _ => {
                                        return Err(Error::custom(
                                            "Invalid entry type in `canisters`",
                                        ));
                                    }
                                };

                                canisters.push(item);
                            }

                            Canisters::Canisters(canisters)
                        } else {
                            return Err(Error::custom("Expected an array for `canisters`"));
                        }
                    },
                    (false, false) => {
                        // Not specified, so default to empty
                        Canisters::Canisters(vec![])
                    },
                };



                Ok(ProjectManifest{
                    canisters,
                    environments: vec![],
                    networks: vec![],
                })
            }
        }

        d.deserialize_map(ProjectManifestVisitor)
    }
}


#[cfg(test)]
mod tests {
    use std::{collections::HashMap};

    use anyhow::{Error, anyhow};
    use indoc::indoc;

    use crate::{
        canister::{build, Settings},
        manifest::{adapter::script, canister::Instructions, environment::CanisterSelection, network::{Managed, Mode}},
    };

    use super::*;

    /// Validates project yaml against the schema and deserializes it to a manifest
    fn validate_project_yaml(s: &str) -> Result<ProjectManifest, Error> {
        let schema = serde_json::from_str::<serde_json::Value>(include_str!("../../../../docs/schemas/icp-yaml-schema.json"))?;
        let project_yaml= serde_yaml::from_str::<serde_json::Value>(s)?;

        // Build & reuse
        let validator = jsonschema::options().build(&schema)?;

        // Iterate over errors
        for error in validator.iter_errors(&project_yaml) {
            eprintln!("Error: {error:#?}");
        }

        assert!(validator.is_valid(&project_yaml));

        Ok(serde_yaml::from_str::<ProjectManifest>(s)?)
    }

    #[test]
    fn validate_manifest_against_schema() -> Result<(), Error> {
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

        "#})?;

        Ok(())
    }


    #[test]
    fn validate_canister_list_manifest() -> Result<(), Error> {
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

        "#})?;

        Ok(())
    }

    #[test]
    fn empty() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<ProjectManifest>(r#""#)?,
            ProjectManifest {
                canisters: Canisters::Canisters(vec![]),
                networks: vec![],
                environments: vec![],
            },
        );

        Ok(())
    }

    #[test]
    fn canister() -> Result<(), Error> {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                canisters:
                  - name: my-canister
                    build:
                      steps:
                        - type: script
                          command: dosomething.sh
            "#})?,
            ProjectManifest {
                canisters: Canisters::Canisters(vec![Item::Manifest(CanisterManifest {
                    name: "my-canister".to_string(),
                    settings: Settings::default(),
                    instructions: Instructions::BuildSync {
                        build: build::Steps {
                            steps: vec![build::Step::Script(script::Adapter {
                                command: script::CommandField::Command(
                                    "dosomething.sh".to_string()
                                )
                            })]
                        },
                        sync: None,
                    },
                })]),
                networks: vec![],
                environments: vec![],
            },
        );

        Ok(())
    }

    #[test]
    fn project_with_invalid_canister_should_fail() -> Result<(), Error> {
        // This canister is invalid because
        match serde_yaml::from_str::<ProjectManifest>(indoc! {r#"
            canisters:
              - name: my-canister
                build:
                  steps: []
        "#})
        {
            Ok(p) => {
                return Err(anyhow!(
                    "A project manifest with an invalid canister manifest should be invalid: {p:?}"
                ));
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains("Failed to load canister manifest: Canister my-canister failed") {
                    return Err(anyhow!(
                        "expected 'Failed to load canister manifest: Canister my-canister failed' error but got: {err}"
                    ));
                }
            }
        };

        Ok(())
    }

    #[test]
    fn canisters_in_list() -> Result<(), Error> {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                canisters:
                  - name: my-canister
                    build:
                      steps:
                        - type: script
                          command: dosomething.sh
            "#})?,
            ProjectManifest {
                canisters: Canisters::Canisters(vec![Item::Manifest(CanisterManifest {
                    name: "my-canister".to_string(),
                    settings: Settings::default(),
                    instructions: Instructions::BuildSync {
                        build: build::Steps {
                            steps: vec![build::Step::Script(script::Adapter {
                                command: script::CommandField::Command(
                                    "dosomething.sh".to_string()
                                )
                            })]
                        },
                        sync: None,
                    },
                })]),
                networks: vec![],
                environments: vec![], 
            },
        );

        Ok(())
    }

    #[test]
    fn canisters_mixed() -> Result<(), Error> {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                canisters:
                  - name: my-canister
                    build:
                      steps:
                        - type: script
                          command: dosomething.sh
                  - canisters/*
            "#})?,
            ProjectManifest {
                canisters: Canisters::Canisters(vec![
                    Item::Manifest(CanisterManifest {
                        name: "my-canister".to_string(),
                        settings: Settings::default(),
                        instructions: crate::manifest::canister::Instructions::BuildSync {
                            build: build::Steps {
                                steps: vec![build::Step::Script(script::Adapter {
                                    command: script::CommandField::Command(
                                        "dosomething.sh".to_string()
                                    )
                                })]
                            },
                            sync: None,
                        },
                    }),
                    Item::Path("canisters/*".to_string())
                ]),
                networks: vec![],
                environments: vec![],
            },
        );

        Ok(())
    }

    #[test]
    fn networks() -> Result<(), Error> {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                networks:
                  - name: my-network
                    mode: managed
            "#})?,
            ProjectManifest {
                canisters: Canisters::Canisters(vec![]),
                networks: vec![Item::Manifest(NetworkManifest {
                    name: "my-network".to_string(),
                    configuration: Mode::Managed (
                         Managed {
                            gateway: None
                        }
                    )}
                )],
                environments: vec![],
            },
        );

        Ok(())
    }

    #[test]
    fn environment() -> Result<(), Error> {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                environments:
                  - name: my-environment
                    network: my-network
                    canisters: [my-canister]
            "#})?,
            ProjectManifest {
                canisters: Canisters::Canisters(vec![]),
                networks: vec![],
                environments: vec![Item::Manifest(
                        EnvironmentManifest {
                            name: "my-environment".to_string(),
                            network: "my-network".to_string(),
                            canisters: CanisterSelection::Named(vec!["my-canister".to_string()]),
                            settings: None,
                    })],
            },
        );

        Ok(())
    }

    #[test]
    fn environment_in_list() -> Result<(), Error> {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                environments:
                  - name: my-environment
                    network: my-network
                    canisters: [my-canister]
            "#})?,
            ProjectManifest {
                canisters: Canisters::Canisters(vec![]),
                networks: vec![],
                environments: vec![Item::Manifest(
                        EnvironmentManifest {
                            name: "my-environment".to_string(),
                            network: "my-network".to_string(),
                            canisters: CanisterSelection::Named(vec!["my-canister".to_string()]),
                            settings: None,
                        })],
            },
        );

        Ok(())
    }

    #[test]
    fn environment_canister_selection() -> Result<(), Error> {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                environments:
                  - name: environment-1
                    canisters: []
                  - name: environment-2
                    canisters: [my-canister]
                  - name: environment-3
            "#})?,
            ProjectManifest {
                canisters: Canisters::Canisters(vec![]),
                networks: vec![],
                environments: vec![
                    Item::Manifest(EnvironmentManifest {
                        name: "environment-1".to_string(),
                        network: "local".to_string(),
                        canisters: CanisterSelection::None,
                        settings: None,
                    }),
                    Item::Manifest(EnvironmentManifest {
                        name: "environment-2".to_string(),
                        network: "local".to_string(),
                        canisters: CanisterSelection::Named(vec!["my-canister".to_string()]),
                        settings: None,
                    }),
                    Item::Manifest(EnvironmentManifest {
                        name: "environment-3".to_string(),
                        network: "local".to_string(),
                        canisters: CanisterSelection::Everything,
                        settings: None,
                    }),
                ],
            },
        );

        Ok(())
    }

    #[test]
    fn environment_settings() -> Result<(), Error> {
        assert_eq!(
            validate_project_yaml(indoc! {r#"
                environments:
                  - name: my-environment
                    settings:
                      canister-1:
                        compute_allocation: 1
                      canister-2:
                        compute_allocation: 2
            "#})?,
            ProjectManifest {
                canisters: Canisters::Canisters(vec![]),
                networks: vec![],
                environments: vec![Item::Manifest(
                    EnvironmentManifest {
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
                    }
                )],
            },
        );

        Ok(())
    }

    #[test]
    fn invalid() -> Result<(), Error> {
        if serde_yaml::from_str::<ProjectManifest>(r#"invalid-content"#).is_ok() {
            return Err(anyhow!(
                "expected invalid manifest to fail deserializeation"
            ));
        }

        Ok(())
    }
}
