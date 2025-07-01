use crate::access::GetNetworkAccessError::{CreateRouteProvider, DecodeRootKey};
use crate::access::NetworkAccessRouting::{RouteProvider, SingleUrl};
use crate::config::model::connected::RouteField;
use crate::{NetworkConfig, NetworkDirectory};
use camino::Utf8PathBuf;
use ic_agent::agent::route_provider::RoundRobinRouteProvider;
use ic_agent::export::Principal;
use ic_agent::{Agent, AgentError, Identity};
use icp_fs::lockedjson::LoadJsonWithLockError;
use snafu::{OptionExt, ResultExt, Snafu};
use std::sync::Arc;
use std::time::Duration;

pub const DEFAULT_IC_GATEWAY: &str = "https://icp0.io";

pub struct NetworkAccess {
    pub default_effective_canister_id: Option<Principal>,
    root_key: Option<Vec<u8>>,
    routing: NetworkAccessRouting,
}

pub enum NetworkAccessRouting {
    SingleUrl(String),
    RouteProvider(Arc<dyn ic_agent::agent::route_provider::RouteProvider>),
}

impl NetworkAccess {
    pub fn new(routing: NetworkAccessRouting) -> Self {
        Self {
            default_effective_canister_id: None,
            root_key: None,
            routing,
        }
    }

    pub fn mainnet() -> Self {
        Self::new(SingleUrl(DEFAULT_IC_GATEWAY.to_string()))
    }

    pub fn create_anonymous_agent(&self) -> Result<Agent, CreateAgentError> {
        let identity = Arc::new(ic_agent::identity::AnonymousIdentity {});
        self.create_agent(identity)
    }

    pub fn create_agent(&self, identity: Arc<dyn Identity>) -> Result<Agent, CreateAgentError> {
        let timeout = expiry_duration();
        let builder = Agent::builder();
        let builder = match &self.routing {
            SingleUrl(url) => builder.with_url(url),
            RouteProvider(route_provider) => {
                builder.with_arc_route_provider(route_provider.clone())
            }
        };
        let agent = builder
            .with_arc_identity(identity) // would come from command-line or config
            .with_ingress_expiry(timeout) // would come from network descriptor
            .build()
            .map_err(|source| CreateAgentError { source })?;
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
    let network_access = match config {
        NetworkConfig::Managed(_managed) => {
            let descriptor =
                nd.load_network_descriptor()?
                    .ok_or(GetNetworkAccessError::NetworkNotRunning {
                        network: nd.network_name().to_string(),
                    })?;
            let port = descriptor.gateway.port;
            if descriptor.gateway.fixed {
                let port_descriptor = nd
                    .load_port_descriptor(port)
                    .context(LoadPortDescriptorSnafu { port })?
                    .context(NoPortDescriptorSnafu { port })?;
                if descriptor.id != port_descriptor.id {
                    return Err(GetNetworkAccessError::NetworkRunningOtherProject {
                        network: nd.network_name().to_string(),
                        port: port_descriptor.gateway.port,
                        project_dir: port_descriptor.project_dir,
                    });
                }
            }

            let url = format!("http://localhost:{port}");
            let default_effective_canister_id = Some(descriptor.default_effective_canister_id);
            let root_key =
                hex::decode(descriptor.root_key).map_err(|source| DecodeRootKey { source })?;

            NetworkAccess {
                default_effective_canister_id,
                root_key: Some(root_key),
                routing: SingleUrl(url),
            }
        }
        NetworkConfig::Connected(connected) => {
            let routing = match &connected.route {
                RouteField::Url(url) => SingleUrl(url.clone()),
                RouteField::Urls(urls) => {
                    let provider = RoundRobinRouteProvider::new(urls.clone())
                        .map_err(|source| CreateRouteProvider { source })?;
                    RouteProvider(Arc::new(provider))
                }
            };
            let root_key = connected
                .root_key
                .as_ref()
                .map(hex::decode)
                .transpose()
                .map_err(|source| DecodeRootKey { source })?;
            NetworkAccess {
                default_effective_canister_id: None,
                root_key,
                routing,
            }
        }
    };
    Ok(network_access)
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
        "this project is configured to use port {port} for the {network} network, but it is already in use by another project at {project_dir}"
    ))]
    NetworkRunningOtherProject {
        network: String,
        port: u16,
        project_dir: Utf8PathBuf,
    },

    #[snafu(display("no descriptor found for port {port}"))]
    NoPortDescriptor { port: u16 },
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to build agent"))]
pub struct CreateAgentError {
    source: AgentError,
}

pub fn expiry_duration() -> Duration {
    // 5 minutes is max ingress timeout
    // 4 minutes accounts for possible replica drift
    Duration::from_secs(60 * 4)
}
