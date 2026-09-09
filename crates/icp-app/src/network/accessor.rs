//! The host's answer to [`icp::network::Access`]: resolving a project's
//! networks against what is actually running on this machine.

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use candid::Principal;
use snafu::{ResultExt, Snafu};
use url::Url;

use icp::manifest::{ProjectRootLocate, ProjectRootLocateError};
use icp::network::{
    Access, AccessError, CollectFriendlyDomains, Configuration, NetworkAccess, NetworkUrls,
};
use icp::prelude::*;
use icp::{CACHE_DIR, ICP_BASE, Network};

use crate::network::{
    NetworkDirectory, custom_domains,
    resolve::{get_connected_network_access, get_managed_network_access, get_managed_network_urls},
};

/// Locating a network's directory needs the project root, which may not be
/// there at all.
#[derive(Debug, Snafu)]
pub enum LocateNetworkDirectoryError {
    #[snafu(display("failed to find project root"))]
    ProjectRootLocate { source: ProjectRootLocateError },
}

/// Where a network keeps its on-disk state.
///
/// Separate from [`Access`] because the directory layout is this crate's
/// invention: the project layer never needs to know a network has one.
pub trait Directories: Send + Sync {
    fn get_network_directory(
        &self,
        network: &Network,
    ) -> Result<NetworkDirectory, LocateNetworkDirectoryError>;
}

#[cfg(any(test, feature = "test-util"))]
/// A [`Directories`] for tests that never look one up.
pub struct UnimplementedMockDirectories;

#[cfg(any(test, feature = "test-util"))]
impl Directories for UnimplementedMockDirectories {
    fn get_network_directory(
        &self,
        _network: &Network,
    ) -> Result<NetworkDirectory, LocateNetworkDirectoryError> {
        unimplemented!("UnimplementedMockDirectories::get_network_directory")
    }
}

pub struct Accessor {
    // Project root
    pub project_root_locate: Arc<dyn ProjectRootLocate>,

    // Port descriptors dir
    pub descriptors: PathBuf,

    // Used to build a bootstrap agent when a connected network fetches its root key
    pub agent: Arc<dyn crate::agent::Create>,
}

impl Accessor {
    /// The network directory is located at `<project_root>/.icp/cache/networks/<network_name>`.
    pub fn get_network_directory(
        &self,
        network: &Network,
    ) -> Result<NetworkDirectory, LocateNetworkDirectoryError> {
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
}

#[async_trait]
impl Directories for Accessor {
    fn get_network_directory(
        &self,
        network: &Network,
    ) -> Result<NetworkDirectory, LocateNetworkDirectoryError> {
        Accessor::get_network_directory(self, network)
    }
}

#[async_trait]
impl Access for Accessor {
    async fn access(&self, network: &Network) -> Result<NetworkAccess, AccessError> {
        match &network.configuration {
            Configuration::Managed { managed: _ } => {
                let nd = self
                    .get_network_directory(network)
                    .map_err(AccessError::new)?;
                get_managed_network_access(nd)
                    .await
                    .map_err(AccessError::new)
            }
            Configuration::Connected { connected: cfg } => {
                get_connected_network_access(cfg, &self.agent)
                    .await
                    .map_err(AccessError::new)
            }
        }
    }

    async fn urls(&self, network: &Network) -> Result<NetworkUrls, AccessError> {
        match &network.configuration {
            Configuration::Managed { managed: _ } => {
                let nd = self
                    .get_network_directory(network)
                    .map_err(AccessError::new)?;
                get_managed_network_urls(nd).await.map_err(AccessError::new)
            }
            // A connected network's endpoints are configured, so there is
            // nothing to resolve.
            Configuration::Connected { connected: cfg } => Ok(NetworkUrls {
                api_url: cfg.api_url.clone(),
                http_gateway_url: cfg.http_gateway_url.clone(),
            }),
        }
    }

    async fn publish_friendly_domains(
        &self,
        network: &Network,
        collect: &CollectFriendlyDomains<'_>,
    ) {
        let Configuration::Managed { .. } = &network.configuration else {
            return;
        };
        let Ok(nd) = self.get_network_directory(network) else {
            return;
        };
        let Ok(Some(desc)) = nd.load_network_descriptor().await else {
            return;
        };
        let Some(status_dir) = &desc.status_dir else {
            return;
        };
        let gateway_url_str = format!("http://{}:{}", desc.gateway.host, desc.gateway.port);
        let Ok(gateway_url) = Url::parse(&gateway_url_str) else {
            tracing::warn!("Failed to parse gateway URL {gateway_url_str:?} for custom domains");
            return;
        };
        let Some(domain) = custom_domains::gateway_domain(&gateway_url) else {
            return;
        };

        // Only here, past every way this can turn out to have nothing to write,
        // is the project asked for any mappings. The descriptor names the
        // network the gateway is actually serving, so it — not the
        // environment's own view — decides which environments share this
        // network and therefore this mapping file.
        let env_entries: BTreeMap<String, Vec<(String, Principal)>> = collect(&desc.network)
            .into_iter()
            .map(|e| (e.environment, e.entries))
            .collect();

        let extra: Vec<_> = custom_domains::ii_custom_domain_entry(desc.ii, domain)
            .into_iter()
            .collect();
        if let Err(e) =
            custom_domains::write_custom_domains(status_dir, domain, &env_entries, &extra)
        {
            tracing::warn!("Failed to update custom domains: {e}");
        }
    }
}
