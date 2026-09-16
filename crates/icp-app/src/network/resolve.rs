//! Resolving a network to something you can talk to.
//!
//! A managed network is described by a descriptor its launcher wrote, so
//! reaching one means reading that file and checking the port is still the one
//! it claims. A connected network carries its own endpoints, and its root key
//! is either pinned or fetched trust-on-first-use.

use std::sync::Arc;

use ic_agent::{AgentError, identity::AnonymousIdentity};
use snafu::{OptionExt, ResultExt, Snafu};
use url::Url;

use icp::network::{Connected, NetworkAccess, NetworkUrls, RootKeySource, RootKeySpec};
use icp::prelude::*;

use crate::{
    agent::{Create, CreateAgentError},
    network::{NetworkDirectory, config::NetworkDescriptorModel, directory::LoadNetworkFileError},
};

#[derive(Debug, Snafu)]
pub enum GetNetworkAccessError {
    #[snafu(display("failed to load port {port} descriptor"))]
    LoadPortDescriptor {
        port: u16,
        source: LoadNetworkFileError,
    },

    #[snafu(display("the {network} network for this project is not running"))]
    NetworkNotRunning { network: String },

    #[snafu(display(
        "port {port} is already in use by the {network} network of another project at {project_dir}"
    ))]
    NetworkRunningOtherProject {
        network: String,
        port: u16,
        project_dir: PathBuf,
    },

    #[snafu(display("no descriptor found for port {port}"))]
    NoPortDescriptor { port: u16 },

    #[snafu(display("failed to load network descriptor"))]
    LoadNetworkDescriptor { source: LoadNetworkFileError },

    #[snafu(display("failed to create agent to fetch root key from {url}"))]
    CreateBootstrapAgent {
        url: Url,
        #[snafu(source(from(CreateAgentError, Box::new)))]
        source: Box<CreateAgentError>,
    },

    #[snafu(display("failed to fetch root key from {url}"))]
    FetchRootKey {
        url: Url,
        #[snafu(source(from(AgentError, Box::new)))]
        source: Box<AgentError>,
    },
}

pub async fn get_managed_network_access(
    nd: NetworkDirectory,
) -> Result<NetworkAccess, GetNetworkAccessError> {
    let (desc, gateway_url) = managed_network_gateway(nd).await?;
    Ok(NetworkAccess {
        root_key: desc.root_key,
        root_key_source: RootKeySource::Managed,
        api_url: gateway_url.clone(),
        http_gateway_url: Some(gateway_url),
        use_friendly_domains: desc.use_friendly_domains,
    })
}

/// The URLs a running managed network is reached at. Its gateway serves the API
/// as well, so both URLs are the same one.
pub async fn get_managed_network_urls(
    nd: NetworkDirectory,
) -> Result<NetworkUrls, GetNetworkAccessError> {
    let (_, gateway_url) = managed_network_gateway(nd).await?;
    Ok(NetworkUrls {
        api_url: gateway_url.clone(),
        http_gateway_url: Some(gateway_url),
    })
}

/// A running managed network's descriptor and the URL its gateway is reachable
/// at. A network that is not running has no descriptor, and one whose fixed port
/// has since been taken by another project's network is not the network the
/// descriptor describes — both are errors rather than a URL nothing answers on.
async fn managed_network_gateway(
    nd: NetworkDirectory,
) -> Result<(NetworkDescriptorModel, Url), GetNetworkAccessError> {
    // Load network descriptor
    let desc = nd
        .load_network_descriptor()
        .await
        .context(LoadNetworkDescriptorSnafu)?
        .ok_or(GetNetworkAccessError::NetworkNotRunning {
            network: nd.network_name.to_owned(),
        })?;

    // Specify port
    let port = desc.gateway.port;

    // Apply gateway configuration
    if desc.gateway.fixed {
        let pdesc = nd
            .load_port_descriptor(port)
            .await
            .context(LoadPortDescriptorSnafu { port })?
            .context(NoPortDescriptorSnafu { port })?;

        if desc.id != pdesc.id {
            return NetworkRunningOtherProjectSnafu {
                network: pdesc.network,
                port: pdesc.gateway.port,
                project_dir: pdesc.project_dir,
            }
            .fail();
        }
    }
    let http_gateway_url = Url::parse(&format!("http://{}:{port}", desc.gateway.host)).unwrap();
    Ok((desc, http_gateway_url))
}

pub async fn get_connected_network_access(
    connected: &Connected,
    agent: &Arc<dyn Create>,
) -> Result<NetworkAccess, GetNetworkAccessError> {
    let (root_key, root_key_source) = match &connected.root_key {
        RootKeySpec::Mainnet => (IC_ROOT_KEY.to_vec(), RootKeySource::Mainnet),
        RootKeySpec::Explicit(bytes) => (bytes.clone(), RootKeySource::Configured),
        RootKeySpec::Fetch => {
            let root_key = fetch_root_key(agent, &connected.api_url).await?;
            (root_key, RootKeySource::Fetched)
        }
    };

    Ok(NetworkAccess {
        root_key,
        root_key_source,
        api_url: connected.api_url.clone(),
        http_gateway_url: connected.http_gateway_url.clone(),
        use_friendly_domains: false,
    })
}

/// Fetch a network's root key trust-on-first-use. This does *not* verify the
/// key's provenance, so we warn the user that responses cannot be trusted the
/// way a pinned key allows.
async fn fetch_root_key(
    agent: &Arc<dyn Create>,
    api_url: &Url,
) -> Result<Vec<u8>, GetNetworkAccessError> {
    tracing::warn!(
        "fetching the root key from {api_url}; its provenance is not verified (trust-on-first-use)"
    );
    let bootstrap = agent
        .create(Arc::new(AnonymousIdentity), api_url.as_str(), None)
        .await
        .context(CreateBootstrapAgentSnafu {
            url: api_url.clone(),
        })?;
    bootstrap
        .fetch_root_key()
        .await
        .context(FetchRootKeySnafu {
            url: api_url.clone(),
        })?;
    Ok(bootstrap.read_root_key())
}
