//! Host-side network facade.
//!
//! The network *configuration* model lives in `icp_deploy_canister::network`
//! and is re-exported here. Runtime access (launching/stopping managed
//! networks, descriptors, agent bootstrap) stays in this crate.

use std::sync::Arc;

use async_trait::async_trait;
use snafu::prelude::*;

pub use icp_deploy_canister::network::*;

pub use access::RootKeySource;
pub use directory::{LoadPidError, NetworkDirectory, SavePidError};
pub use managed::run::{RunNetworkError, run_network};

use crate::{
    CACHE_DIR, ICP_BASE, Network,
    manifest::{ProjectRootLocate, ProjectRootLocateError},
    network::access::{
        GetNetworkAccessError, NetworkAccess, get_connected_network_access,
        get_managed_network_access,
    },
    prelude::*,
};

pub mod access;
pub mod config;
pub mod custom_domains;
pub mod directory;
pub mod managed;

#[derive(Debug, Snafu)]
pub enum AccessError {
    #[snafu(display("failed to find project root"))]
    ProjectRootLocate { source: ProjectRootLocateError },

    #[snafu(transparent)]
    GetNetworkAccess { source: GetNetworkAccessError },
}

#[async_trait]
pub trait Access: Sync + Send {
    fn get_network_directory(&self, network: &Network) -> Result<NetworkDirectory, AccessError>;
    async fn access(&self, network: &Network) -> Result<NetworkAccess, AccessError>;
}

pub struct Accessor {
    // Project root
    pub project_root_locate: Arc<dyn ProjectRootLocate>,

    // Port descriptors dir
    pub descriptors: PathBuf,

    // Used to build a bootstrap agent when a connected network fetches its root key
    pub agent: Arc<dyn crate::agent::Create>,
}

#[async_trait]
impl Access for Accessor {
    /// The network directory is located at `<project_root>/.icp/cache/networks/<network_name>`.
    fn get_network_directory(&self, network: &Network) -> Result<NetworkDirectory, AccessError> {
        let dir = self
            .project_root_locate
            .locate()
            .context(ProjectRootLocateSnafu)?;
        Ok(NetworkDirectory::new(
            &network.name,
            &dir.join(ICP_BASE)
                .join(CACHE_DIR)
                .join("networks")
                .join(&network.name),
            &self.descriptors,
        ))
    }
    async fn access(&self, network: &Network) -> Result<NetworkAccess, AccessError> {
        match &network.configuration {
            Configuration::Managed { managed: _ } => {
                let nd = self.get_network_directory(network)?;
                Ok(get_managed_network_access(nd).await?)
            }
            Configuration::Connected { connected: cfg } => {
                Ok(get_connected_network_access(cfg, &self.agent).await?)
            }
        }
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
    fn get_network_directory(&self, network: &Network) -> Result<NetworkDirectory, AccessError> {
        Ok(NetworkDirectory {
            network_name: network.name.clone(),
            network_root: PathBuf::new(),
            port_descriptor_dir: PathBuf::new(),
        })
    }
    async fn access(&self, network: &Network) -> Result<NetworkAccess, AccessError> {
        self.networks
            .get(&network.name)
            .cloned()
            .ok_or_else(|| AccessError::GetNetworkAccess {
                source: GetNetworkAccessError::NetworkNotRunning {
                    network: network.name.clone(),
                },
            })
    }
}
