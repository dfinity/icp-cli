use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

use crate::manifest::{
    Item, canister::CanisterManifest, environment::EnvironmentManifest, network::NetworkManifest,
};

#[derive(Debug, PartialEq, Deserialize, JsonSchema)]
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

impl Networks {
    pub fn with_defaults(self) -> Self {
        Self::Networks(
            [
                Into::<Vec<NetworkManifest>>::into(Self::default()),
                Into::<Vec<NetworkManifest>>::into(self),
            ]
            .concat(),
        )
    }
}

impl From<Networks> for Vec<NetworkManifest> {
    fn from(value: Networks) -> Self {
        match value {
            Networks::Network(v) => vec![v],
            Networks::Networks(items) => items,
        }
    }
}

#[derive(Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Environments {
    Environment(EnvironmentManifest),
    Environments(Vec<EnvironmentManifest>),
}

impl Environments {
    pub fn with_defaults(self) -> Self {
        Self::Environments(
            [
                Into::<Vec<EnvironmentManifest>>::into(Self::default()),
                Into::<Vec<EnvironmentManifest>>::into(self),
            ]
            .concat(),
        )
    }
}

impl From<Environments> for Vec<EnvironmentManifest> {
    fn from(value: Environments) -> Self {
        match value {
            Environments::Environment(v) => vec![v],
            Environments::Environments(items) => items,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ProjectInner {
    #[serde(flatten)]
    pub canisters: Option<Canisters>,

    #[serde(flatten)]
    pub networks: Option<Networks>,

    #[serde(flatten)]
    pub environments: Option<Environments>,
}

#[derive(Debug, PartialEq)]
pub struct ProjectManifest {
    pub canisters: Vec<Item<CanisterManifest>>,
    pub networks: Vec<NetworkManifest>,
    pub environments: Vec<EnvironmentManifest>,
}

impl From<ProjectInner> for ProjectManifest {
    fn from(v: ProjectInner) -> Self {
        let ProjectInner {
            canisters,
            networks,
            environments,
        } = v;

        // Canisters
        let canisters = canisters.unwrap_or_default().into();

        // Networks
        let networks = match networks {
            // None specified, use defaults
            None => Networks::default().into(),

            // Network(s) specified, append to default
            Some(vs) => [
                Into::<Vec<NetworkManifest>>::into(Networks::default()),
                Into::<Vec<NetworkManifest>>::into(vs),
            ]
            .concat(),
        };

        // Environments
        let environments = match environments {
            // None specified, use defaults
            None => Environments::default().into(),

            // Environment(s) specified, append to default
            Some(vs) => [
                Into::<Vec<EnvironmentManifest>>::into(Environments::default()),
                Into::<Vec<EnvironmentManifest>>::into(vs),
            ]
            .concat(),
        };

        Self {
            canisters,
            networks,
            environments,
        }
    }
}

impl<'de> Deserialize<'de> for ProjectManifest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let inner: ProjectInner = Deserialize::deserialize(d)?;
        Ok(inner.into())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use anyhow::{Error, anyhow};
    use indoc::indoc;

    use crate::{
        canister::{Settings, build, sync},
        manifest::{adapter::script, canister::Instructions, environment::CanisterSelection},
        network::Configuration,
    };

    use super::*;

    #[test]
    fn empty() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<ProjectManifest>(r#""#)?,
            ProjectManifest {
                canisters: Canisters::default().into(),
                networks: Networks::default().into(),
                environments: Environments::default().into(),
            },
        );

        Ok(())
    }

    #[test]
    fn canister() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<ProjectManifest>(indoc! {r#"
                canister:
                  name: my-canister
                  build:
                    steps:
                      - type: script
                        command: dosomething.sh
            "#})?,
            ProjectManifest {
                canisters: vec![Item::Manifest(CanisterManifest {
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
                        sync: sync::Steps { steps: vec![] },
                    },
                })],
                networks: Networks::default().into(),
                environments: Environments::default().into(),
            },
        );

        Ok(())
    }

    #[test]
    fn project_with_invalid_canister_should_fail() -> Result<(), Error> {
        // This canister is invalid because
        match serde_yaml::from_str::<ProjectManifest>(indoc! {r#"
            canister:
              name: my-canister
              build:
                steps: []
        "#})
        {
            Ok(_) => {
                return Err(anyhow!(
                    "A project manifest with an invalid canister manifest should be invalid"
                ));
            }
            Err(err) => {
                let err_msg = format!("{err}");
                if !err_msg.contains("Canister my-canister is invalid") {
                    return Err(anyhow!(
                        "expected 'Canister my-canister is invalid' error but got: {err}"
                    ));
                }
            }
        };

        Ok(())
    }

    #[test]
    fn canisters_in_list() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<ProjectManifest>(indoc! {r#"
                canisters:
                  - name: my-canister
                    build:
                      steps:
                        - type: script
                          command: dosomething.sh
            "#})?,
            ProjectManifest {
                canisters: vec![Item::Manifest(CanisterManifest {
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
                        sync: sync::Steps { steps: vec![] },
                    },
                })],
                networks: Networks::default().into(),
                environments: Environments::default().into(),
            },
        );

        Ok(())
    }

    #[test]
    fn canisters_mixed() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<ProjectManifest>(indoc! {r#"
                canisters:
                  - name: my-canister
                    build:
                      steps:
                        - type: script
                          command: dosomething.sh
                  - canisters/*
            "#})?,
            ProjectManifest {
                canisters: vec![
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
                            sync: sync::Steps { steps: vec![] },
                        },
                    }),
                    Item::Path("canisters/*".to_string())
                ],
                networks: Networks::default().into(),
                environments: Environments::default().into(),
            },
        );

        Ok(())
    }

    #[test]
    fn network() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<ProjectManifest>(indoc! {r#"
                network:
                  name: my-network
            "#})?,
            ProjectManifest {
                canisters: Canisters::default().into(),
                networks: Networks::Networks(vec![NetworkManifest {
                    name: "my-network".to_string(),
                    configuration: Configuration::default(),
                }])
                .with_defaults()
                .into(),
                environments: Environments::default().into(),
            },
        );

        Ok(())
    }

    #[test]
    fn networks_in_list() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<ProjectManifest>(indoc! {r#"
                networks:
                  - name: my-network
            "#})?,
            ProjectManifest {
                canisters: Canisters::default().into(),
                networks: Networks::Networks(vec![NetworkManifest {
                    name: "my-network".to_string(),
                    configuration: Configuration::default(),
                }])
                .with_defaults()
                .into(),
                environments: Environments::default().into(),
            },
        );

        Ok(())
    }

    #[test]
    fn environment() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<ProjectManifest>(indoc! {r#"
                environment:
                  name: my-environment
                  network: my-network
                  canisters: [my-canister]
            "#})?,
            ProjectManifest {
                canisters: Canisters::default().into(),
                networks: Networks::default().into(),
                environments: Environments::Environments(vec![EnvironmentManifest {
                    name: "my-environment".to_string(),
                    network: "my-network".to_string(),
                    canisters: CanisterSelection::Named(vec!["my-canister".to_string()]),
                    settings: None,
                }])
                .with_defaults()
                .into(),
            },
        );

        Ok(())
    }

    #[test]
    fn environment_in_list() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<ProjectManifest>(indoc! {r#"
                environments:
                  - name: my-environment
                    network: my-network
                    canisters: [my-canister]
            "#})?,
            ProjectManifest {
                canisters: Canisters::default().into(),
                networks: Networks::default().into(),
                environments: Environments::Environments(vec![EnvironmentManifest {
                    name: "my-environment".to_string(),
                    network: "my-network".to_string(),
                    canisters: CanisterSelection::Named(vec!["my-canister".to_string()]),
                    settings: None,
                }])
                .with_defaults()
                .into(),
            },
        );

        Ok(())
    }

    #[test]
    fn environment_canister_selection() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<ProjectManifest>(
                r#"
                environments:
                  - name: environment-1
                    canisters: []
                  - name: environment-2
                    canisters: [my-canister]
                  - name: environment-3
                "#
            )?,
            ProjectManifest {
                canisters: Canisters::default().into(),
                networks: Networks::default().into(),
                environments: Environments::Environments(vec![
                    EnvironmentManifest {
                        name: "environment-1".to_string(),
                        network: "local".to_string(),
                        canisters: CanisterSelection::None,
                        settings: None,
                    },
                    EnvironmentManifest {
                        name: "environment-2".to_string(),
                        network: "local".to_string(),
                        canisters: CanisterSelection::Named(vec!["my-canister".to_string()]),
                        settings: None,
                    },
                    EnvironmentManifest {
                        name: "environment-3".to_string(),
                        network: "local".to_string(),
                        canisters: CanisterSelection::Everything,
                        settings: None,
                    }
                ])
                .with_defaults()
                .into(),
            },
        );

        Ok(())
    }

    #[test]
    fn environment_settings() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<ProjectManifest>(
                r#"
                environment:
                  name: my-environment
                  settings:
                    canister-1:
                      compute_allocation: 1
                    canister-2:
                      compute_allocation: 2
                "#
            )?,
            ProjectManifest {
                canisters: Canisters::default().into(),
                networks: Networks::default().into(),
                environments: Environments::Environments(vec![EnvironmentManifest {
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
                }])
                .with_defaults()
                .into(),
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
