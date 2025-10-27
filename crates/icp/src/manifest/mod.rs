use crate::{
    network::{Configuration, Connected, Managed},
    prelude::*,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::manifest::{
    environment::CanisterSelection,
    project::{Canisters, Environments, Networks},
};

pub(crate) mod adapter;
pub(crate) mod canister;
pub(crate) mod environment;
pub(crate) mod network;
pub mod project;
pub(crate) mod recipe;
pub(crate) mod serde_helpers;

pub use {canister::CanisterManifest, environment::EnvironmentManifest, network::NetworkManifest};

pub const PROJECT_MANIFEST: &str = "icp.yaml";
pub const CANISTER_MANIFEST: &str = "canister.yaml";

#[derive(Clone, Debug, PartialEq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Item<T> {
    /// Path to a manifest
    Path(String),

    /// The manifest
    Manifest(T),
}

impl Default for Canisters {
    fn default() -> Self {
        Canisters::Canisters(vec![Item::Path("canisters/*".into())])
    }
}

impl Default for Networks {
    fn default() -> Self {
        Networks::Networks(vec![
            Item::Manifest(NetworkManifest {
                name: "local".to_string(),
                configuration: Configuration::Managed {
                    managed: Managed::default(),
                },
            }),
            Item::Manifest(NetworkManifest {
                name: "mainnet".to_string(),
                configuration: Configuration::Connected {
                    connected: Connected {
                        url: IC_MAINNET_NETWORK_URL.to_string(),
                        root_key: None,
                    },
                },
            }),
        ])
    }
}

impl Default for Environments {
    fn default() -> Self {
        Environments::Environments(vec![Item::Manifest(EnvironmentManifest {
            name: "local".to_string(),
            network: "local".to_string(),
            canisters: CanisterSelection::Everything,
            settings: None,
        })])
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LocateError {
    #[error("project manifest not found in {0}")]
    NotFound(PathBuf),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub trait Locate: Sync + Send {
    fn locate(&self) -> Result<PathBuf, LocateError>;
}

pub struct Locator {
    /// Current directory to begin search from in case dir is unspecified.
    cwd: PathBuf,

    /// Specific directory to look in (overrides `cwd`).
    dir: Option<PathBuf>,
}

impl Locator {
    pub fn new(cwd: PathBuf, dir: Option<PathBuf>) -> Self {
        Self { cwd, dir }
    }
}

impl Locate for Locator {
    fn locate(&self) -> Result<PathBuf, LocateError> {
        // Specified path
        if let Some(dir) = &self.dir {
            if !dir.join(PROJECT_MANIFEST).exists() {
                return Err(LocateError::NotFound(dir.to_owned()));
            }

            return Ok(dir.to_owned());
        }

        // Unspecified path
        let mut dir = self.cwd.to_owned();

        loop {
            if !dir.join(PROJECT_MANIFEST).exists() {
                if let Some(p) = dir.parent() {
                    dir = p.to_path_buf();
                    continue;
                }

                return Err(LocateError::NotFound(self.cwd.to_owned()));
            }

            return Ok(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Error;
    use indoc::indoc;
    use serde_json::Value;

    use crate::network::{Gateway, Port};

    use super::*;

    fn load_icp_yaml_schema() -> Result<Value, Error> {
        Ok(serde_json::from_str::<serde_json::Value>(include_str!("../../../../docs/schemas/icp-yaml-schema.json"))?)
    }

    #[test]
    fn default_canisters() -> Result<(), Error> {
        assert_eq!(
            Canisters::default(),
            Canisters::Canisters(vec![Item::Path("canisters/*".into())])
        );

        Ok(())
    }

    #[test]
    fn default_networks() -> Result<(), Error> {
        assert_eq!(
            Networks::default(),
            Networks::Networks(vec![
                Item::Manifest(NetworkManifest {
                    name: "local".to_string(),
                    configuration: Configuration::Managed {
                        managed: Managed {
                            gateway: Gateway {
                                host: "localhost".to_string(),
                                port: Port::Fixed(8000),
                            },
                        }
                    },
                }),
                Item::Manifest(NetworkManifest {
                    name: "mainnet".to_string(),
                    configuration: Configuration::Connected {
                        connected: Connected {
                            url: "https://icp-api.io".to_string(),
                            root_key: None,
                        }
                    },
                }),
            ])
        );

        Ok(())
    }


    #[test]
    fn default_environments() -> Result<(), Error> {
        assert_eq!(
            Environments::default(),
            Environments::Environments(vec![Item::Manifest(EnvironmentManifest {
                name: "local".to_string(),
                network: "local".to_string(),
                canisters: CanisterSelection::Everything,
                settings: None,
            })])
        );

        Ok(())
    }

    #[test]
    fn validate_manifest_against_schema() -> Result<(), Error> {
        let schema = load_icp_yaml_schema()?;
        let icp_yaml= serde_yaml::from_str::<serde_json::Value>(indoc! {r#"
            canister:
              name: my-canister

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

        assert!(jsonschema::is_valid(&schema, &icp_yaml));
        Ok(())
    }


    #[test]
    fn validate_canister_list_manifest() -> Result<(), Error> {
        let schema = load_icp_yaml_schema()?;
        let icp_yaml= serde_yaml::from_str::<serde_json::Value>(indoc! {r#"
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

        let validator = jsonschema::validator_for(&schema)?;
        for error in validator.iter_errors(&icp_yaml) {
            eprintln!("Error Kind: {:?}", error.kind);
            eprintln!("Error path: {}", error.schema_path);
            eprintln!("Error: {}", error);
            eprintln!("Location: {}", error.instance_path);
        }

        assert!(validator.is_valid(&icp_yaml));

        Ok(())
    }
}
