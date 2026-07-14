use std::sync::Arc;

use ic_agent::{AgentError, identity::AnonymousIdentity};
use serde::Serialize;
use snafu::{OptionExt, ResultExt, Snafu};
use url::Url;

use crate::{
    agent::{Create, CreateAgentError},
    context::IC_ROOT_KEY,
    manifest::network::RootKeySpec,
    network::{Connected, NetworkDirectory, directory::LoadNetworkFileError},
    prelude::*,
};

/// Where a network's root key came from. Used for display so users can tell a
/// trusted/pinned key apart from one that was fetched trust-on-first-use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RootKeySource {
    /// Key belongs to a managed network we launched.
    Managed,
    /// The canonical IC mainnet root key.
    Mainnet,
    /// An explicit key pinned in the manifest or on the command line.
    Configured,
    /// Fetched from the network (trust-on-first-use, provenance unverified).
    Fetched,
}

#[derive(Clone)]
pub struct NetworkAccess {
    /// Network's (resolved) root key.
    pub root_key: Vec<u8>,

    /// Where [`Self::root_key`] came from.
    pub root_key_source: RootKeySource,

    /// Routing configuration
    pub api_url: Url,
    pub http_gateway_url: Option<Url>,

    /// If true, use friendly canister names with the gateway url
    pub use_friendly_domains: bool,
}

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
    Ok(NetworkAccess {
        root_key: desc.root_key,
        root_key_source: RootKeySource::Managed,
        api_url: http_gateway_url.clone(),
        http_gateway_url: Some(http_gateway_url),
        use_friendly_domains: desc.use_friendly_domains,
    })
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
        .create(Arc::new(AnonymousIdentity), api_url.as_str())
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
