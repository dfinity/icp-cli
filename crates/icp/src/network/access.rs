use ic_agent::export::Principal;
use snafu::{OptionExt, ResultExt, Snafu};

use crate::{
    Network,
    fs::json,
    network::{Configuration, NetworkDirectory, access::GetNetworkAccessError::DecodeRootKey},
    prelude::*,
};

pub struct NetworkAccess {
    /// Effective canister ID corresponding to a subnet
    pub default_effective_canister_id: Option<Principal>,

    /// Network's root-key
    pub root_key: Option<Vec<u8>>,

    /// Routing configuration
    pub url: String,
}

impl NetworkAccess {
    pub fn new(url: &str) -> Self {
        Self {
            default_effective_canister_id: None,
            root_key: None,
            url: url.into(),
        }
    }
}

impl NetworkAccess {
    pub fn mainnet() -> Self {
        Self::new(IC_MAINNET_NETWORK_URL)
    }
}

#[derive(Debug, Snafu)]
pub enum GetNetworkAccessError {
    #[snafu(display("failed to decode root key"))]
    DecodeRootKey { source: hex::FromHexError },

    #[snafu(transparent)]
    LoadJsonWithLock { source: json::Error },

    #[snafu(display("failed to load port {port} descriptor"))]
    LoadPortDescriptor { port: u16, source: json::Error },

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
}

pub fn get_network_access(
    nd: NetworkDirectory,
    network: &Network,
) -> Result<NetworkAccess, GetNetworkAccessError> {
    let access = match &network.configuration {
        //
        // Managed
        Configuration::Managed(_) => {
            // Load network descriptor
            let desc =
                nd.load_network_descriptor()?
                    .ok_or(GetNetworkAccessError::NetworkNotRunning {
                        network: nd.network_name.to_owned(),
                    })?;

            // Specify port
            let port = desc.gateway.port;

            // Apply gateway configuration
            if desc.gateway.fixed {
                let pdesc = nd
                    .load_port_descriptor(port)
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

            NetworkAccess {
                default_effective_canister_id,
                root_key: Some(root_key),
                url: format!("http://localhost:{port}"),
            }
        }

        //
        // Connected
        Configuration::Connected(cfg) => {
            let root_key = cfg
                .root_key
                .as_ref()
                .map(hex::decode)
                .transpose()
                .map_err(|err| DecodeRootKey { source: err })?;

            NetworkAccess {
                default_effective_canister_id: None,
                root_key,
                url: cfg.url.to_owned(),
            }
        }
    };

    Ok(access)
}
