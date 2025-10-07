use std::sync::Arc;

use anyhow::Context as _;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer};

pub use directory::NetworkDirectory;
pub use managed::run::{RunNetworkError, run_network};

use crate::{
    Network,
    manifest::{Locate, LocateError},
    network::access::{NetworkAccess, get_network_access},
    prelude::*,
};

pub mod access;
pub mod config;
mod directory;
mod lock;
mod managed;
pub mod structure;

pub const DEFAULT_IC_GATEWAY: &str = "https://icp0.io";

#[derive(Clone, Debug, PartialEq, JsonSchema)]
pub enum Port {
    Fixed(u16),
    Random,
}

impl Default for Port {
    fn default() -> Self {
        Port::Fixed(8080)
    }
}

impl<'de> Deserialize<'de> for Port {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match u16::deserialize(d)? {
            0 => Port::Random,
            p => Port::Fixed(p),
        })
    }
}

fn default_host() -> String {
    "localhost".to_string()
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct Gateway {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default)]
    pub port: Port,
}

impl Default for Gateway {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: Default::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema)]
pub struct Managed {
    pub gateway: Gateway,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub struct Connected {
    /// The URL this network can be reached at.
    pub url: String,

    /// The root key of this network
    pub root_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Configuration {
    /// A managed network is one which can be controlled and manipulated.
    Managed(Managed),

    /// A connected network is one which can be interacted with
    /// but cannot be controlled or manipulated.
    Connected(Connected),
}

impl Default for Configuration {
    fn default() -> Self {
        Configuration::Managed(Managed::default())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AccessError {
    #[error("failed to find project root")]
    Project(#[from] LocateError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Access: Sync + Send {
    async fn access(&self, network: &Network) -> Result<NetworkAccess, AccessError>;
}

pub struct Accessor {
    // Project root locator
    pub project: Arc<dyn Locate>,

    // Port descriptors dir
    pub descriptors: PathBuf,
}

#[async_trait]
impl Access for Accessor {
    async fn access(&self, network: &Network) -> Result<NetworkAccess, AccessError> {
        // Locate networks directory
        let dir = self.project.locate()?;

        // NetworkDirectory
        let nd = NetworkDirectory::new(
            &network.name,                                          // name
            &dir.join(".icp").join("networks").join(&network.name), // network_root
            &self.descriptors,                                      // port_descriptor_dir
        );

        // NetworkAccess
        let acceess = get_network_access(nd, network).context("failed to load network access")?;

        Ok(acceess)
    }
}
