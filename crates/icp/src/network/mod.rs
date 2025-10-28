use std::sync::Arc;

use anyhow::Context as _;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

pub use directory::{LoadPidError, NetworkDirectory, SavePidError};
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
pub mod managed;
pub mod structure;

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

#[cfg(any(test, feature = "test-features"))]
use std::collections::HashMap;

#[cfg(any(test, feature = "test-features"))]
/// Mock network access provider for testing.
///
/// Allows configuring network access details for specific networks.
/// Supports a default fallback for networks not explicitly configured.
pub struct MockNetworkAccessor {
    /// Default network access to return for unconfigured networks
    default: NetworkAccess,

    /// Network-specific access configurations by network name
    networks: HashMap<String, NetworkAccess>,
}

#[cfg(any(test, feature = "test-features"))]
impl MockNetworkAccessor {
    /// Creates a new mock network accessor with the given default.
    pub fn new(default: NetworkAccess) -> Self {
        Self {
            default,
            networks: HashMap::new(),
        }
    }

    /// Creates a mock with localhost:8000 as the default.
    pub fn localhost() -> Self {
        Self::new(NetworkAccess::new("http://localhost:8000"))
    }

    /// Adds a network-specific access configuration.
    pub fn with_network(mut self, name: impl Into<String>, access: NetworkAccess) -> Self {
        self.networks.insert(name.into(), access);
        self
    }

    /// Sets the default network access.
    pub fn with_default(mut self, access: NetworkAccess) -> Self {
        self.default = access;
        self
    }
}

#[cfg(any(test, feature = "test-features"))]
#[async_trait]
impl Access for MockNetworkAccessor {
    async fn access(&self, network: &Network) -> Result<NetworkAccess, AccessError> {
        Ok(self
            .networks
            .get(&network.name)
            .cloned()
            .unwrap_or_else(|| self.default.clone()))
    }
}
