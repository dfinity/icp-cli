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

#[derive(Debug, PartialEq, JsonSchema)]
pub struct ProjectManifest {
    #[serde(flatten)]
    #[schemars(with = "Option<Canisters>")]
    pub canisters: Vec<Item<CanisterManifest>>,

    #[schemars(with = "Option<NetworkManifest>")]
    pub networks: Vec<NetworkManifest>,

    #[schemars(with = "Option<EnvironmentManifest>")]
    pub environments: Vec<EnvironmentManifest>,
}

impl<'de> Deserialize<'de> for ProjectManifest {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};
        use std::fmt;

        struct ProjectManifestVisitor;

        impl<'de> Visitor<'de> for ProjectManifestVisitor {
            type Value = ProjectManifest;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str(
                    "a project manifest with canister, network and environment definitions",
                )
            }

            // We're going to build the project manifest manually
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

                // Start with canister definitions
                // We need to handle:
                // - canister - a single manifest
                // - canisters - a list of manifests and/or paths

                let canister_key = serde_yaml::Value::String("canister".to_string());
                let canisters_key = serde_yaml::Value::String("canisters".to_string());

                let has_canister = top_map.contains_key(&canister_key);
                let has_canisters = top_map.contains_key(&canisters_key);

                let canisters: Vec<Item<CanisterManifest>> = match (has_canister, has_canisters) {
                    (true, true) => {
                        // This is an invalid case

                        return Err(Error::custom(
                            "Project cannot define both `canister` and `canisters` sections",
                        ));
                    }

                    (true, false) => {
                        // There is a single inline canister manifest

                        let canister_value = top_map
                            .remove(&canister_key)
                            .ok_or_else(|| Error::custom("Invalid `canister` key"))?;

                        let canister_manifest: CanisterManifest =
                            serde_yaml::from_value(canister_value).map_err(|e| {
                                Error::custom(format!("Failed to load canister manifest: {}", e))
                            })?;

                        Canisters::Canister(canister_manifest).into()
                    }

                    (false, true) => {
                        // We have a list of Canisters

                        if let serde_yaml::Value::Sequence(seq) = top_map
                            .remove(&canisters_key)
                            .ok_or_else(|| Error::custom("`canisters` key does not exist"))?
                        {
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

                            canisters
                        } else {
                            return Err(Error::custom("Expected an array for `canisters`"));
                        }
                    }

                    (false, false) => {
                        // No canister definition, we use the default
                        Canisters::default().into()
                    }
                };

                // Deserialize the environments, we support:
                // - no environments defined, in which case we end up with the defaults
                // - environment - a single environment is defined
                // - environments - a list of environments are defined
                let environment_key = serde_yaml::Value::String("environment".to_string());
                let environments_key = serde_yaml::Value::String("environments".to_string());

                let has_environment = top_map.contains_key(&environment_key);
                let has_environments = top_map.contains_key(&environments_key);

                let environments: Vec<EnvironmentManifest> = match (
                    has_environment,
                    has_environments,
                ) {
                    (true, true) => {
                        // This is an invalid case

                        return Err(Error::custom(
                            "Project cannot define both `environment` and `environments` sections",
                        ));
                    }
                    (true, false) => {
                        // Single environment defined

                        let environment_value = top_map
                            .remove(&environment_key)
                            .ok_or_else(|| Error::custom("Invalid `environment` key"))?;

                        let environment_manifest: EnvironmentManifest =
                            serde_yaml::from_value(environment_value).map_err(|e| {
                                Error::custom(format!("Failed to load environment manifest: {}", e))
                            })?;

                        [
                            Into::<Vec<EnvironmentManifest>>::into(Environments::default()),
                            vec![environment_manifest],
                        ]
                        .concat()
                    }
                    (false, true) => {
                        if let serde_yaml::Value::Sequence(seq) = top_map
                            .remove(&environments_key)
                            .ok_or_else(|| Error::custom("'environments' key does not exist"))?
                        {
                            let mut environments: Vec<EnvironmentManifest> =
                                Vec::with_capacity(seq.len());

                            for v in seq {
                                let environment_manifest =
                                    serde_yaml::from_value(v).map_err(|e| {
                                        Error::custom(format!("Failed to load environment: {}", e))
                                    })?;

                                environments.push(environment_manifest);
                            }

                            [
                                Into::<Vec<EnvironmentManifest>>::into(Environments::default()),
                                environments,
                            ]
                            .concat()
                        } else {
                            return Err(Error::custom("Expected an array for `environments`"));
                        }
                    }
                    (false, false) => Environments::default().into(),
                };

                // Deserialize the networks, we support:
                // - no networks defined, in which case we end up with the defaults
                // - network - a single network is defined
                // - networks - a list of networks are defined
                let network_key = serde_yaml::Value::String("network".to_string());
                let networks_key = serde_yaml::Value::String("networks".to_string());

                let has_network = top_map.contains_key(&network_key);
                let has_networks = top_map.contains_key(&networks_key);

                let networks: Vec<NetworkManifest> = match (has_network, has_networks) {
                    (true, true) => {
                        // This is an invalid case

                        return Err(Error::custom(
                            "Project cannot define both `network` and `networks` sections",
                        ));
                    }
                    (true, false) => {
                        // Single network defined

                        let network_value = top_map
                            .remove(&network_key)
                            .ok_or_else(|| Error::custom("Invalid `network` key"))?;

                        let network_manifest: NetworkManifest =
                            serde_yaml::from_value(network_value).map_err(|e| {
                                Error::custom(format!("Failed to load network manifest: {}", e))
                            })?;

                        [
                            Into::<Vec<NetworkManifest>>::into(Networks::default()),
                            vec![network_manifest],
                        ]
                        .concat()
                    }
                    (false, true) => {
                        if let serde_yaml::Value::Sequence(seq) = top_map
                            .remove(&networks_key)
                            .ok_or_else(|| Error::custom("'networks' key does not exist"))?
                        {
                            let mut networks: Vec<NetworkManifest> = Vec::with_capacity(seq.len());

                            for v in seq {
                                let network_manifest = serde_yaml::from_value(v).map_err(|e| {
                                    Error::custom(format!("Failed to load network: {}", e))
                                })?;

                                networks.push(network_manifest);
                            }

                            [
                                Into::<Vec<NetworkManifest>>::into(Networks::default()),
                                networks,
                            ]
                            .concat()
                        } else {
                            return Err(Error::custom("Expected an array for `networks`"));
                        }
                    }
                    (false, false) => Networks::default().into(),
                };

                Ok(ProjectManifest {
                    canisters,
                    networks,
                    environments,
                })
            }
        }

        d.deserialize_map(ProjectManifestVisitor)
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
                if !err_msg.contains("Canister my-canister failed to parse") {
                    return Err(anyhow!(
                        "expected 'Canister my-canister failed to parse' error but got: {err}"
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
