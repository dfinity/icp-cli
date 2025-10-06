use crate::prelude::*;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::manifest::{
    environment::CanisterSelection,
    network::{Configuration, Gateway},
    project::{Canisters, Environments, Networks},
};

pub mod adapter;
pub mod canister;
pub mod environment;
pub mod network;
pub mod project;
pub mod recipe;

pub use {canister::CanisterManifest, environment::EnvironmentManifest, network::NetworkManifest};

pub const PROJECT_MANIFEST: &str = "icp.yaml";
pub const CANISTER_MANIFEST: &str = "icp.yaml";

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
            NetworkManifest {
                name: "local".to_string(),
                configuration: Configuration::Managed(network::Managed {
                    gateway: Gateway {
                        host: "localhost".to_string(),
                        port: network::Port::Fixed(8080),
                    },
                }),
            },
            NetworkManifest {
                name: "mainnet".to_string(),
                configuration: Configuration::Connected(network::Connected {
                    url: "https://ic0.app".to_string(),
                    root_key: None,
                }),
            },
        ])
    }
}

impl Default for Environments {
    fn default() -> Self {
        Environments::Environments(vec![EnvironmentManifest {
            name: "local".to_string(),
            network: "local".to_string(),
            canisters: CanisterSelection::Everything,
            settings: None,
        }])
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LocateError {
    #[error("project manifest not found in {0}")]
    NoManifest(PathBuf),

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
                return Err(LocateError::NoManifest(dir.to_owned()));
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

                return Err(LocateError::NoManifest(self.cwd.to_owned()));
            }

            return Ok(dir);
        }
    }
}
