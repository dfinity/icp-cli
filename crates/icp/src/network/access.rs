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
    let http_gateway_url = Url::parse(&format!("http://localhost:{port}")).unwrap();
    Ok(NetworkAccess {
        root_key: desc.root_key,
        api_url: http_gateway_url.clone(),
        http_gateway_url: Some(http_gateway_url),
    })
}

pub async fn get_connected_network_access(
    connected: &Connected,
) -> Result<NetworkAccess, GetNetworkAccessError> {
    let root_key = connected.root_key.clone();

    Ok(NetworkAccess {
        root_key,
        api_url: connected.api_url.clone(),
        http_gateway_url: connected.http_gateway_url.clone(),
    })
}
