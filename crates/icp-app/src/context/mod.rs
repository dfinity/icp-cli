use std::sync::Arc;
use url::Url;

use crate::{
    agent::CreateAgentError, directories, identity::IdentitySelection, telemetry_data::NetworkType,
};
use candid::Principal;
use ic_agent::{Agent, Identity};
use icp_project::{
    host::{
        CanisterSelection, EnvironmentSelection, GetCanisterIdForEnvError, GetEnvironmentError,
        Host,
    },
    network::{Configuration as NetworkConfiguration, NetworkAccess, RootKeySpec},
    prelude::*,
};
use snafu::{OptionExt, ResultExt, Snafu};
use time::OffsetDateTime;

mod init;

pub use init::initialize;

/// Selection type for networks - similar to IdentitySelection
#[derive(Clone, Debug, PartialEq)]
pub enum NetworkSelection {
    /// Use the network from the environment
    Default,
    /// Use a named network
    Named(String),
    /// Use a network by URL
    Url(Url, RootKeySpec),
}

/// Selection type for network commands that accept either network name or environment
#[derive(Clone, Debug, PartialEq)]
pub enum NetworkOrEnvironmentSelection {
    /// Use a network by name
    Network(String),
    /// Use the network from an environment by name
    Environment(String),
}

#[derive(Clone)]
pub struct Context {
    /// The project-side resources operations run against.
    pub host: Host,

    /// Various cli-related directories (cache, configuration, etc).
    pub dirs: Arc<dyn directories::Access>,

    /// Where a network keeps its on-disk state. Not part of
    /// [`icp_project::network::Access`] because the layout is this crate's invention.
    pub network_dirs: Arc<dyn crate::network::Directories>,

    /// Identity loader
    identity: Arc<dyn crate::identity::Load>,

    /// Agent creator
    agent: Arc<dyn crate::agent::Create>,

    /// Whether debug is enabled
    pub debug: bool,

    /// Telemetry data collected during command execution
    pub telemetry_data: Arc<crate::telemetry_data::TelemetryData>,

    /// Password reader for identity decryption; shared with the identity loader.
    pub password_func: Arc<dyn Fn() -> Result<String, String> + Send + Sync>,
}

impl Context {
    /// Gets an identity based on the provided identity selection.
    // TODO: refactor the whole codebase to use this method instead of directly accessing `ctx.identity.load()`
    pub async fn get_identity(
        &self,
        identity: &IdentitySelection,
        network_root_key: Option<Vec<u8>>,
    ) -> Result<Arc<dyn Identity>, GetIdentityError> {
        self.identity
            .load(identity.clone(), network_root_key)
            .await
            .context(IdentityLoadSnafu {
                identity: identity.clone(),
            })
    }

    /// Gets an Network by name from the currently loaded project.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be loaded or if the network is not found.
    pub async fn get_network(
        &self,
        network_selection: &NetworkSelection,
    ) -> Result<icp_project::Network, GetNetworkError> {
        let network = match network_selection {
            NetworkSelection::Named(network_name) => {
                if self.host.project.exists().await? {
                    let p = self.host.project.load().await?;
                    let net = p.networks.get(network_name).context(NetworkNotFoundSnafu {
                        name: network_name.to_owned(),
                    })?;
                    net.clone()
                } else if network_name == IC {
                    icp_project::Network {
                        name: IC.to_string(),
                        configuration: icp_project::network::Configuration::Connected {
                            connected: icp_project::network::Connected {
                                api_url: IC_MAINNET_NETWORK_API_URL.parse().unwrap(),
                                http_gateway_url: Some(
                                    IC_MAINNET_NETWORK_GATEWAY_URL.parse().unwrap(),
                                ),
                                root_key: RootKeySpec::Mainnet,
                            },
                        },
                    }
                } else {
                    return Err(GetNetworkError::NetworkNotFound {
                        name: network_name.to_owned(),
                    });
                }
            }
            NetworkSelection::Default => return Err(GetNetworkError::DefaultNetwork),
            NetworkSelection::Url(url, root_key) => icp_project::Network {
                name: url.to_string(),
                configuration: icp_project::network::Configuration::Connected {
                    connected: icp_project::network::Connected {
                        api_url: url.clone(),
                        http_gateway_url: Some(url.clone()),
                        root_key: root_key.clone(),
                    },
                },
            },
        };

        let network_type = match &network.configuration {
            NetworkConfiguration::Managed { .. } => NetworkType::Managed,
            NetworkConfiguration::Connected { .. } => NetworkType::Connected,
        };
        self.telemetry_data.set_network_type(network_type);

        Ok(network)
    }

    /// Gets a network from either a network name or environment name.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be loaded or if the network/environment is not found.
    pub async fn get_network_or_environment(
        &self,
        selection: &NetworkOrEnvironmentSelection,
    ) -> Result<icp_project::Network, GetNetworkOrEnvironmentError> {
        match selection {
            NetworkOrEnvironmentSelection::Network(network_name) => {
                let network_selection = NetworkSelection::Named(network_name.clone());
                Ok(self.get_network(&network_selection).await?)
            }
            NetworkOrEnvironmentSelection::Environment(env_name) => {
                let env_selection = EnvironmentSelection::Named(env_name.clone());
                let env = self.host.get_environment(&env_selection).await?;
                Ok(env.network)
            }
        }
    }

    /// Creates an agent for a given identity and environment.
    pub async fn get_agent_for_env(
        &self,
        identity: &IdentitySelection,
        environment: &EnvironmentSelection,
    ) -> Result<Agent, GetAgentForEnvError> {
        let env = self.host.get_environment(environment).await?;
        let access = self.host.network.access(&env.network).await?;
        let id = self
            .get_identity(identity, Some(access.root_key.clone()))
            .await?;
        Ok(self.create_agent(id, access).await?)
    }

    /// Creates an agent for a given identity and network.
    pub async fn get_agent_for_network(
        &self,
        identity: &IdentitySelection,
        network_selection: &NetworkSelection,
    ) -> Result<Agent, GetAgentForNetworkError> {
        let network = self.get_network(network_selection).await?;
        let access = self.host.network.access(&network).await?;
        let id = self
            .get_identity(identity, Some(access.root_key.clone()))
            .await?;
        Ok(self.create_agent(id, access).await?)
    }

    /// Private helper to create an agent given identity and network access.
    ///
    /// Used by [`Self::get_agent_for_env`] and [`Self::get_agent_for_network`].
    async fn create_agent(
        &self,
        id: Arc<dyn Identity>,
        network_access: NetworkAccess,
    ) -> Result<Agent, CreateAgentError> {
        let agent = self
            .agent
            .create(id, network_access.api_url.as_str(), None)
            .await?;
        agent.set_root_key(network_access.root_key);
        Ok(agent)
    }

    /// Creates an agent for a given identity and url.
    pub async fn get_agent_for_url(
        &self,
        identity: &IdentitySelection,
        url: &Url,
    ) -> Result<Agent, GetAgentForUrlError> {
        let id = self.get_identity(identity, None).await?;
        let agent = self.agent.create(id, url.as_str(), None).await?;
        Ok(agent)
    }

    /// Creates an agent for signing a message that a different machine will submit.
    ///
    /// Unlike the other constructors this resolves no root key: nothing is
    /// verified on this side, and resolving one can mean a network round trip the
    /// signing machine is unable to make. The agent's ingress expiry is pinned so
    /// that the expiry it derives on its own — for the pre-signed
    /// `request_status` that accompanies an update — lands exactly on
    /// `expire_at`, the instant the call envelope itself expires.
    pub async fn get_agent_for_signing(
        &self,
        identity: &IdentitySelection,
        url: &Url,
        expire_at: OffsetDateTime,
    ) -> Result<Agent, GetAgentForSigningError> {
        // Loading the identity can block on a password prompt or a hardware
        // token, so measure what is left of the window only once it is in hand.
        let id = self.get_identity(identity, None).await?;
        let remaining = (expire_at - OffsetDateTime::now_utc())
            .try_into()
            .ok()
            .context(SigningWindowClosedSnafu { expire_at })?;
        Ok(self.agent.create(id, url.as_str(), Some(remaining)).await?)
    }

    pub async fn get_agent(
        &self,
        identity: &IdentitySelection,
        network: &NetworkSelection,
        environment: &EnvironmentSelection,
    ) -> Result<Agent, GetAgentError> {
        match (environment, network) {
            // Error: Both environment and network specified
            (EnvironmentSelection::Named(_), NetworkSelection::Named(_))
            | (EnvironmentSelection::Named(_), NetworkSelection::Url(_, _)) => {
                Err(GetAgentError::EnvironmentAndNetworkSpecified)
            }

            // Default environment + default network
            (EnvironmentSelection::Default, NetworkSelection::Default) => {
                // Try to get agent from the default environment if project exists
                match self.get_agent_for_env(identity, environment).await {
                    Ok(agent) => Ok(agent),
                    Err(GetAgentForEnvError::GetEnvironment {
                        source:
                            GetEnvironmentError::ProjectLoad {
                                source: icp_project::ProjectLoadError::Locate { .. },
                            },
                    }) => Err(GetAgentError::NoProjectOrNetwork),
                    Err(e) => Err(e.into()),
                }
            }

            // Environment specified
            (EnvironmentSelection::Named(_), NetworkSelection::Default) => {
                Ok(self.get_agent_for_env(identity, environment).await?)
            }

            // Network specified
            (EnvironmentSelection::Default, NetworkSelection::Named(_))
            | (EnvironmentSelection::Default, NetworkSelection::Url(_, _)) => {
                Ok(self.get_agent_for_network(identity, network).await?)
            }
        }
    }

    pub async fn get_canister_id(
        &self,
        canister: &CanisterSelection,
        network: &NetworkSelection,
        environment: &EnvironmentSelection,
    ) -> Result<Principal, GetCanisterIdError> {
        match canister {
            CanisterSelection::Principal(principal) => Ok(*principal),
            CanisterSelection::Named(_) => {
                match (environment, network) {
                    // Error: Both environment and network specified
                    (EnvironmentSelection::Named(_), NetworkSelection::Named(_))
                    | (EnvironmentSelection::Named(_), NetworkSelection::Url(_, _)) => {
                        Err(GetCanisterIdError::CanisterEnvironmentAndNetworkSpecified)
                    }

                    // Error: Canister by name with explicit network but no environment
                    (EnvironmentSelection::Default, NetworkSelection::Named(_))
                    | (EnvironmentSelection::Default, NetworkSelection::Url(_, _)) => {
                        Err(GetCanisterIdError::AmbiguousCanisterName)
                    }

                    // Only environment specified
                    (_, NetworkSelection::Default) => Ok(self
                        .host
                        .get_canister_id_for_env(canister, environment)
                        .await?),
                }
            }
        }
    }

    #[cfg(test)]
    /// Creates a test context with all mocks
    pub fn mocked() -> Context {
        Context {
            host: Host::mocked(),
            telemetry_data: Arc::new(crate::telemetry_data::TelemetryData::default()),
            dirs: Arc::new(crate::directories::UnimplementedMockDirs),
            network_dirs: Arc::new(crate::network::UnimplementedMockDirectories),
            identity: Arc::new(crate::identity::MockIdentityLoader::anonymous()),
            agent: Arc::new(crate::agent::Creator),
            debug: false,
            password_func: Arc::new(|| Err("no password available in mock context".to_string())),
        }
    }
}

#[derive(Debug, Snafu)]
pub enum GetIdentityError {
    #[snafu(display("failed to load identity"))]
    IdentityLoad {
        source: crate::identity::LoadError,
        identity: IdentitySelection,
    },
}

#[derive(Debug, Snafu)]
pub enum GetNetworkError {
    #[snafu(transparent)]
    ProjectLoad {
        source: icp_project::ProjectLoadError,
    },

    #[snafu(display("project does not contain a network named '{}'", name))]
    NetworkNotFound { name: String },

    #[snafu(display("cannot load URL-specified network"))]
    UrlSpecifiedNetwork,

    #[snafu(display("cannot load default network"))]
    DefaultNetwork,
}

#[derive(Debug, Snafu)]
pub enum GetNetworkOrEnvironmentError {
    #[snafu(transparent)]
    NetworkResolution { source: GetNetworkError },

    #[snafu(transparent)]
    EnvironmentResolution { source: GetEnvironmentError },
}

#[derive(Debug, Snafu)]
pub enum GetAgentForEnvError {
    #[snafu(transparent)]
    GetIdentity { source: GetIdentityError },

    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(transparent)]
    NetworkAccess {
        source: icp_project::network::AccessError,
    },

    #[snafu(transparent)]
    AgentCreate {
        source: crate::agent::CreateAgentError,
    },
}

#[derive(Debug, Snafu)]
pub enum GetAgentForNetworkError {
    #[snafu(transparent)]
    GetIdentity { source: GetIdentityError },

    #[snafu(transparent)]
    GetNetwork { source: GetNetworkError },

    #[snafu(transparent)]
    NetworkAccess {
        source: icp_project::network::AccessError,
    },

    #[snafu(transparent)]
    AgentCreate {
        source: crate::agent::CreateAgentError,
    },
}

#[derive(Debug, Snafu)]
pub enum GetAgentForUrlError {
    #[snafu(transparent)]
    GetIdentity { source: GetIdentityError },

    #[snafu(transparent)]
    AgentCreate {
        source: crate::agent::CreateAgentError,
    },
}

#[derive(Debug, Snafu)]
pub enum GetAgentForSigningError {
    #[snafu(transparent)]
    GetIdentity { source: GetIdentityError },

    #[snafu(display(
        "the submission window closed at {expire_at} before the message could be signed"
    ))]
    SigningWindowClosed { expire_at: OffsetDateTime },

    #[snafu(transparent)]
    AgentCreate {
        source: crate::agent::CreateAgentError,
    },
}

#[derive(Debug, Snafu)]
pub enum GetAgentError {
    #[snafu(transparent)]
    ProjectExists {
        source: icp_project::ProjectLoadError,
    },

    #[snafu(display("You can't specify both an environment and a network"))]
    EnvironmentAndNetworkSpecified,

    #[snafu(display(
        "No project found and no network specified. Either run this command inside a project or specify a network with --network"
    ))]
    NoProjectOrNetwork,

    #[snafu(transparent)]
    GetAgentForEnv { source: GetAgentForEnvError },

    #[snafu(transparent)]
    GetAgentForNetwork { source: GetAgentForNetworkError },

    #[snafu(transparent)]
    GetAgentForUrl { source: GetAgentForUrlError },
}

#[derive(Debug, Snafu)]
pub enum GetCanisterIdError {
    #[snafu(display("You can't specify both an environment and a network"))]
    CanisterEnvironmentAndNetworkSpecified,

    #[snafu(display(
        "Specifying a network is not supported if you are targeting a canister by name, specify an environment instead"
    ))]
    AmbiguousCanisterName,

    #[snafu(transparent)]
    GetCanisterIdForEnv { source: GetCanisterIdForEnvError },
}

#[cfg(test)]
mod tests;
