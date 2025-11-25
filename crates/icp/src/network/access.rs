use ic_agent::export::Principal;
use snafu::{OptionExt, ResultExt, Snafu};
use url::Url;

use crate::{
    network::{
        Connected, NetworkDirectory, access::GetNetworkAccessError::DecodeRootKey,
        directory::LoadNetworkFileError,
    },
    prelude::*,
};

#[derive(Clone)]
pub struct NetworkAccess {
    /// Effective canister ID corresponding to a subnet
    pub default_effective_canister_id: Option<Principal>,

    /// Network's root-key
    pub root_key: Option<Vec<u8>>,

    /// Routing configuration
    pub url: Url,
}

impl NetworkAccess {
    pub fn new(url: &Url) -> Self {
        Self {
            default_effective_canister_id: None,
            root_key: None,
            url: url.clone(),
        }
    }
}

impl NetworkAccess {
    pub fn mainnet() -> Self {
        Self::new(&Url::parse(IC_MAINNET_NETWORK_URL).unwrap())
    }
}

#[derive(Debug, Snafu)]
pub enum GetNetworkAccessError {
    #[snafu(display("failed to decode root key"))]
    DecodeRootKey { source: hex::FromHexError },

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
    #[snafu(display("failed to parse URL {url}"))]
    ParseUrl {
        url: String,
        source: url::ParseError,
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
            return Err(GetNetworkAccessError::NetworkRunningOtherProject {
                network: pdesc.network,
                port: pdesc.gateway.port,
                project_dir: pdesc.project_dir,
            });
        }
    }

    // Specify effective canister ID
    let default_effective_canister_id = Some(desc.default_effective_canister_id);

    // Specify root-key
    let root_key = hex::decode(desc.root_key).map_err(|source| DecodeRootKey { source })?;

    Ok(NetworkAccess {
        default_effective_canister_id,
        root_key: Some(root_key),
        url: Url::parse(&format!("http://localhost:{port}")).unwrap(),
    })
}

pub async fn get_connected_network_access(
    connected: &Connected,
) -> Result<NetworkAccess, GetNetworkAccessError> {
    let root_key = connected
        .root_key
        .as_ref()
        .map(hex::decode)
        .transpose()
        .map_err(|err| DecodeRootKey { source: err })?;

    Ok(NetworkAccess {
        default_effective_canister_id: None,
        root_key,
        url: Url::parse(&connected.url).context(ParseUrlSnafu {
            url: connected.url.clone(),
        })?,
    })
}
