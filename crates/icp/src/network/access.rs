use snafu::{OptionExt, ResultExt, Snafu};
use url::Url;

use crate::{
    network::{Connected, NetworkDirectory, directory::LoadNetworkFileError},
    prelude::*,
};

#[derive(Clone)]
pub struct NetworkAccess {
    /// Network's root-key
    pub root_key: Vec<u8>,

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

    #[snafu(display(
        "a root key is required to connect to remote network `{url}`; pass `--root-key` (or the manifest `root-key`), or use `--network ic` for mainnet"
    ))]
    RootKeyRequiredForRemote { url: Url },

    #[snafu(display("failed to build agent to fetch the root key from local network `{url}`"))]
    BuildRootKeyAgent {
        url: Url,
        #[snafu(source(from(ic_agent::AgentError, Box::new)))]
        source: Box<ic_agent::AgentError>,
    },

    #[snafu(display("failed to fetch the root key from local network `{url}`"))]
    FetchRootKey {
        url: Url,
        #[snafu(source(from(ic_agent::AgentError, Box::new)))]
        source: Box<ic_agent::AgentError>,
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
        api_url: http_gateway_url.clone(),
        http_gateway_url: Some(http_gateway_url),
        use_friendly_domains: desc.use_friendly_domains,
    })
}

pub async fn get_connected_network_access(
    connected: &Connected,
) -> Result<NetworkAccess, GetNetworkAccessError> {
    let root_key = match &connected.root_key {
        Some(rk) => rk.clone(),
        None => resolve_root_key(&connected.api_url).await?,
    };

    Ok(NetworkAccess {
        root_key,
        api_url: connected.api_url.clone(),
        http_gateway_url: connected.http_gateway_url.clone(),
        use_friendly_domains: false,
    })
}

/// Whether `url`'s host is a loopback address (or `localhost`).
fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// Resolve the root key for a connected network whose key was omitted.
///
/// For a local (loopback) network the key is fetched from the network's status
/// endpoint — safe because there is no meaningful man-in-the-middle surface on
/// loopback. For a remote network we refuse rather than silently defaulting to
/// the mainnet key: a genuine remote network never shares mainnet's key, so
/// defaulting would only defer the failure to the first certified call with a
/// cryptic verification error.
async fn resolve_root_key(api_url: &Url) -> Result<Vec<u8>, GetNetworkAccessError> {
    if !is_loopback(api_url) {
        return RootKeyRequiredForRemoteSnafu {
            url: api_url.clone(),
        }
        .fail();
    }

    let agent = ic_agent::Agent::builder()
        .with_url(api_url.as_str())
        .build()
        .context(BuildRootKeyAgentSnafu {
            url: api_url.clone(),
        })?;
    agent.fetch_root_key().await.context(FetchRootKeySnafu {
        url: api_url.clone(),
    })?;
    Ok(agent.read_root_key())
}
