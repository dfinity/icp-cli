use std::sync::Arc;
use std::time::Duration;

use ic_agent::{Agent, AgentError, Identity, export::Principal};
use icp::prelude::*;
use icp_fs::lockedjson::LoadJsonWithLockError;
use snafu::{OptionExt, ResultExt, Snafu};

use crate::access::GetNetworkAccessError::DecodeRootKey;
use crate::{NetworkConfig, NetworkDirectory};

pub const DEFAULT_IC_GATEWAY: &str = "https://icp0.io";

pub const SECOND: u64 = 1;
pub const MINUTE: u64 = 60 * SECOND;

pub struct NetworkAccess {
    /// Effective canister ID corresponding to a subnet
    pub default_effective_canister_id: Option<Principal>,

    /// Network's root-key
    root_key: Option<Vec<u8>>,

    /// Routing configuration
    url: String,
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
        Self::new(DEFAULT_IC_GATEWAY)
    }
}

impl NetworkAccess {
    pub fn create_agent(&self, identity: Arc<dyn Identity>) -> Result<Agent, CreateAgentError> {
        let builder = Agent::builder();

        // Specify url
        let builder = builder.with_url(&self.url);

        // Create agent
        let agent = builder
            .with_arc_identity(identity)
            .with_ingress_expiry(Duration::from_secs(4 * MINUTE))
            .build()
            .map_err(|err| CreateAgentError { source: err })?;

        // Set root-key
        if let Some(root_key) = &self.root_key {
            agent.set_root_key(root_key.clone());
        }

        Ok(agent)
    }
}

pub fn get_network_access(
    nd: NetworkDirectory,
    config: &NetworkConfig,
) -> Result<NetworkAccess, GetNetworkAccessError> {
    Ok(match config {
        // Managed
        NetworkConfig::Managed(_) => {
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

        // Connected
        NetworkConfig::Connected(connected) => {
            let root_key = connected
                .root_key
                .as_ref()
                .map(hex::decode)
                .transpose()
                .map_err(|err| DecodeRootKey { source: err })?;

            NetworkAccess {
                default_effective_canister_id: None,
                root_key,
                url: connected.url.to_owned(),
            }
        }
    })
}

#[derive(Debug, Snafu)]
pub enum GetNetworkAccessError {
    #[snafu(display("failed to create route provider"))]
    CreateRouteProvider { source: AgentError },

    #[snafu(display("failed to decode root key"))]
    DecodeRootKey { source: hex::FromHexError },

    #[snafu(transparent)]
    LoadJsonWithLock { source: LoadJsonWithLockError },

    #[snafu(display("failed to load port {port} descriptor"))]
    LoadPortDescriptor {
        port: u16,
        source: LoadJsonWithLockError,
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
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to build agent"))]
pub struct CreateAgentError {
    source: AgentError,
}
