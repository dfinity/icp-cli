use std::sync::Arc;

use anyhow::Context as _;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

pub use directory::{LoadPidError, NetworkDirectory, SavePidError};
pub use managed::run::{RunNetworkError, run_network};

use crate::{
    Network,
    manifest::{
        Locate, LocateError,
        network::{Connected as ManifestConnected, Gateway as ManifestGateway, Mode},
    },
    network::access::{NetworkAccess, get_network_access},
    prelude::*,
};

pub mod access;
pub mod config;
mod directory;
// mod lock;
pub mod managed;

#[derive(Clone, Debug, PartialEq, JsonSchema, Serialize)]
pub enum Port {
    Fixed(u16),
    Random,
}

impl Default for Port {
    fn default() -> Self {
        Port::Fixed(8000)
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

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
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

#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Managed {
    pub gateway: Gateway,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Connected {
    /// The URL this network can be reached at.
    pub url: String,

    /// The root key of this network
    pub root_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Configuration {
    // Note: we must use struct variants to be able to flatten
    // and make schemars generate the proper schema
    /// A managed network is one which can be controlled and manipulated.
    Managed {
        #[serde(flatten)]
        managed: Managed,
    },

    /// A connected network is one which can be interacted with
    /// but cannot be controlled or manipulated.
    Connected {
        #[serde(flatten)]
        connected: Connected,
    },
}

impl Default for Configuration {
    fn default() -> Self {
        Configuration::Managed {
            managed: Managed::default(),
        }
    }
}

impl From<ManifestGateway> for Gateway {
    fn from(value: ManifestGateway) -> Self {
        let host = value.host.unwrap_or("localhost".to_string());
        let port = match value.port {
            Some(0) => Port::Random,
            Some(p) => Port::Fixed(p),
            None => Port::Random,
        };
        Gateway { host, port }
    }
}

impl From<ManifestConnected> for Connected {
    fn from(value: ManifestConnected) -> Self {
        let url = value.url.clone();
        let root_key = value.root_key;
        Connected { url, root_key }
    }
}

impl From<Mode> for Configuration {
    fn from(value: Mode) -> Self {
        match value {
            Mode::Managed(managed) => {
                let gateway: Gateway = match managed.gateway {
                    Some(g) => g.into(),
                    None => Gateway::default(),
                };

                Configuration::Managed {
                    managed: Managed { gateway },
                }
            }
            Mode::Connected(connected) => Configuration::Connected {
                connected: connected.into(),
            },
        }
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
        let access = get_network_access(nd, network)
            .await
            .context("failed to load network access")?;

        Ok(access)
    }
}

#[cfg(test)]
use std::collections::HashMap;

#[cfg(test)]
pub struct MockNetworkAccessor {
    /// Network-specific access configurations by network name
    networks: HashMap<String, NetworkAccess>,
}

#[cfg(test)]
impl MockNetworkAccessor {
    /// Creates a new empty mock network accessor.
    pub fn new() -> Self {
        Self {
            networks: HashMap::new(),
        }
    }

    /// Adds a network-specific access configuration.
    pub fn with_network(mut self, name: impl Into<String>, access: NetworkAccess) -> Self {
        self.networks.insert(name.into(), access);
        self
    }
}

#[cfg(test)]
impl Default for MockNetworkAccessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[async_trait]
impl Access for MockNetworkAccessor {
    async fn access(&self, network: &Network) -> Result<NetworkAccess, AccessError> {
        self.networks.get(&network.name).cloned().ok_or_else(|| {
            AccessError::Unexpected(anyhow::anyhow!(
                "network '{}' not configured in mock",
                network.name
            ))
        })
    }
}
