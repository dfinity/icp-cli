use serde::{Deserialize, Deserializer};

use crate::manifest::{Item, canister::Canister, environment::Environment, network::Network};

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(clippy::large_enum_variant)]
pub enum Canisters {
    Canister(Canister),
    Canisters(Vec<Item<Canister>>),
}

impl From<Canisters> for Vec<Item<Canister>> {
    fn from(value: Canisters) -> Self {
        match value {
            Canisters::Canister(v) => vec![Item::Manifest(v)],
            Canisters::Canisters(items) => items,
        }
    }
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Networks {
    Network(Network),
    Networks(Vec<Item<Network>>),
}

impl Networks {
    pub fn with_defaults(self) -> Self {
        Self::Networks(
            [
                Into::<Vec<Item<Network>>>::into(Self::default()),
                Into::<Vec<Item<Network>>>::into(self),
            ]
            .concat(),
        )
    }
}

impl From<Networks> for Vec<Item<Network>> {
    fn from(value: Networks) -> Self {
        match value {
            Networks::Network(v) => vec![Item::Manifest(v)],
            Networks::Networks(items) => items,
        }
    }
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environments {
    Environment(Environment),
    Environments(Vec<Environment>),
}

impl Environments {
    pub fn with_defaults(self) -> Self {
        Self::Environments(
            [
                Into::<Vec<Environment>>::into(Self::default()),
                Into::<Vec<Environment>>::into(self),
            ]
            .concat(),
        )
    }
}

impl From<Environments> for Vec<Environment> {
    fn from(value: Environments) -> Self {
        match value {
            Environments::Environment(v) => vec![v],
            Environments::Environments(items) => items,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ProjectInner {
    #[serde(flatten)]
    pub canisters: Option<Canisters>,

    #[serde(flatten)]
    pub networks: Option<Networks>,

    #[serde(flatten)]
    pub environments: Option<Environments>,
}

#[derive(Debug, PartialEq)]
pub struct Project {
    pub canisters: Vec<Item<Canister>>,
    pub networks: Vec<Item<Network>>,
    pub environments: Vec<Environment>,
}

impl From<ProjectInner> for Project {
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
                Into::<Vec<Item<Network>>>::into(Networks::default()),
                Into::<Vec<Item<Network>>>::into(vs),
            ]
            .concat(),
        };

        // Environments
        let environments = match environments {
            // None specified, use defaults
            None => Environments::default().into(),

            // Environment(s) specified, append to default
            Some(vs) => [
                Into::<Vec<Environment>>::into(Environments::default()),
                Into::<Vec<Environment>>::into(vs),
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

impl<'de> Deserialize<'de> for Project {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let inner: ProjectInner = Deserialize::deserialize(d)?;
        Ok(inner.into())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use anyhow::Error;

    use crate::{
        canister::{Settings, build, sync},
        manifest::{canister::Instructions, environment::CanisterSelection, network::Mode},
    };

    use super::*;

    #[test]
    fn empty() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Project>(r#""#)?,
            Project {
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
            serde_yaml::from_str::<Project>(
                r#"
                canister:
                  name: my-canister
                  build:
                    steps: []
                "#
            )?,
            Project {
                canisters: vec![Item::Manifest(Canister {
                    name: "my-canister".to_string(),
                    settings: Settings::default(),
                    instructions: Instructions::BuildSync {
                        build: build::Steps { steps: vec![] },
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
    fn canisters_in_list() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Project>(
                r#"
                canisters:
                  - name: my-canister
                    build:
                      steps: []
                "#
            )?,
            Project {
                canisters: vec![Item::Manifest(Canister {
                    name: "my-canister".to_string(),
                    settings: Settings::default(),
                    instructions: Instructions::BuildSync {
                        build: build::Steps { steps: vec![] },
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
            serde_yaml::from_str::<Project>(
                r#"
                canisters:
                  - name: my-canister
                    build:
                      steps: []
                  - canisters/*
                "#
            )?,
            Project {
                canisters: vec![
                    Item::Manifest(Canister {
                        name: "my-canister".to_string(),
                        settings: Settings::default(),
                        instructions: crate::manifest::canister::Instructions::BuildSync {
                            build: build::Steps { steps: vec![] },
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
            serde_yaml::from_str::<Project>(
                r#"
                network:
                  name: my-network
                  mode: managed
                "#
            )?,
            Project {
                canisters: Canisters::default().into(),
                networks: Networks::Networks(vec![Item::Manifest(Network {
                    name: "my-network".to_string(),
                    mode: Mode::Managed,
                }),])
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
            serde_yaml::from_str::<Project>(
                r#"
                networks:
                  - name: my-network
                    mode: managed
                "#
            )?,
            Project {
                canisters: Canisters::default().into(),
                networks: Networks::Networks(vec![Item::Manifest(Network {
                    name: "my-network".to_string(),
                    mode: Mode::Managed,
                }),])
                .with_defaults()
                .into(),
                environments: Environments::default().into(),
            },
        );

        Ok(())
    }

    #[test]
    fn networks_mixed() -> Result<(), Error> {
        assert_eq!(
            serde_yaml::from_str::<Project>(
                r#"
                networks:
                  - networks/*
                  - name: my-network
                    mode: managed
                "#
            )?,
            Project {
                canisters: Canisters::default().into(),
                networks: Networks::Networks(vec![
                    Item::Path("networks/*".to_string()),
                    Item::Manifest(Network {
                        name: "my-network".to_string(),
                        mode: Mode::Managed,
                    }),
                ])
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
            serde_yaml::from_str::<Project>(
                r#"
                environment:
                  name: my-environment
                  network: my-network
                  canisters: [my-canister]
                "#
            )?,
            Project {
                canisters: Canisters::default().into(),
                networks: Networks::default().into(),
                environments: Environments::Environments(vec![Environment {
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
            serde_yaml::from_str::<Project>(
                r#"
                environments:
                  - name: my-environment
                    network: my-network
                    canisters: [my-canister]
                "#
            )?,
            Project {
                canisters: Canisters::default().into(),
                networks: Networks::default().into(),
                environments: Environments::Environments(vec![Environment {
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
            serde_yaml::from_str::<Project>(
                r#"
                environments:
                  - name: environment-1
                    canisters: []
                  - name: environment-2
                    canisters: [my-canister]
                  - name: environment-3
                "#
            )?,
            Project {
                canisters: Canisters::default().into(),
                networks: Networks::default().into(),
                environments: Environments::Environments(vec![
                    Environment {
                        name: "environment-1".to_string(),
                        network: "local".to_string(),
                        canisters: CanisterSelection::None,
                        settings: None,
                    },
                    Environment {
                        name: "environment-2".to_string(),
                        network: "local".to_string(),
                        canisters: CanisterSelection::Named(vec!["my-canister".to_string()]),
                        settings: None,
                    },
                    Environment {
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
            serde_yaml::from_str::<Project>(
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
            Project {
                canisters: Canisters::default().into(),
                networks: Networks::default().into(),
                environments: Environments::Environments(vec![Environment {
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
}
