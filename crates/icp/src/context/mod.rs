use console::Term;
use std::sync::Arc;
use url::Url;

use crate::{
    Canister,
    agent::CreateAgentError,
    canister::{build::Build, sync::Synchronize},
    directories,
    identity::IdentitySelection,
    network::{Configuration as NetworkConfiguration, access::NetworkAccess},
    prelude::*,
    project::{
        DEFAULT_LOCAL_ENVIRONMENT_NAME, DEFAULT_MAINNET_NETWORK_NAME, DEFAULT_MAINNET_NETWORK_URL,
    },
    store_id::{IdMapping, LookupIdError},
};
use candid::Principal;
use ic_agent::{Agent, Identity};
use snafu::{OptionExt, ResultExt, Snafu};

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
    Url(Url),
}

/// Selection type for environments - similar to IdentitySelection
#[derive(Clone, Debug, PartialEq)]
pub enum EnvironmentSelection {
    /// Use the default environment (local)
    Default,
    /// Use a named environment
    Named(String),
}

impl EnvironmentSelection {
    pub fn name(&self) -> &str {
        match self {
            EnvironmentSelection::Default => DEFAULT_LOCAL_ENVIRONMENT_NAME,
            EnvironmentSelection::Named(name) => name,
        }
    }
}

/// Selection type for canisters - similar to IdentitySelection
#[derive(Clone, Debug, PartialEq)]
pub enum CanisterSelection {
    /// Use a canister by name (requires project context)
    Named(String),
    /// Use a canister by principal
    Principal(Principal),
}

pub struct Context {
    /// Terminal for printing messages for the user to see
    pub term: Term,

    /// Various cli-related directories (cache, configuration, etc).
    pub dirs: Arc<dyn directories::Access>,

    /// Canisters ID Store for lookup and storage
    pub ids: Arc<dyn crate::store_id::Access>,

    /// An artifact store for canister build artifacts
    pub artifacts: Arc<dyn crate::store_artifact::Access>,

    /// Project loader
    pub project: Arc<dyn crate::ProjectLoad>,

    /// Identity loader
    identity: Arc<dyn crate::identity::Load>,

    /// NetworkAccess loader
    pub network: Arc<dyn crate::network::Access>,

    /// Agent creator
    agent: Arc<dyn crate::agent::Create>,

    /// Canister builder
    pub builder: Arc<dyn Build>,

    /// Canister synchronizer
    pub syncer: Arc<dyn Synchronize>,

    /// Whether debug is enabled
    pub debug: bool,
}

impl Context {
    /// Gets an identity based on the provided identity selection.
    // TODO: refactor the whole codebase to use this method instead of directly accessing `ctx.identity.load()`
    pub async fn get_identity(
        &self,
        identity: &IdentitySelection,
    ) -> Result<Arc<dyn Identity>, GetIdentityError> {
        self.identity
            .load(identity.clone())
            .await
            .context(IdentityLoadSnafu {
                identity: identity.clone(),
            })
    }

    /// Gets an environment by name from the currently loaded project.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be loaded or if the environment is not found.
    pub async fn get_environment(
        &self,
        environment: &EnvironmentSelection,
    ) -> Result<crate::Environment, GetEnvironmentError> {
        // Load project
        let p = self.project.load().await?;

        // Load target environment
        let env = p
            .environments
            .get(environment.name())
            .context(EnvironmentNotFoundSnafu {
                name: environment.name().to_owned(),
            })?;

        Ok(env.clone())
    }

    /// Gets an Network by name from the currently loaded project.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be loaded or if the network is not found.
    pub async fn get_network(
        &self,
        network_selection: &NetworkSelection,
    ) -> Result<crate::Network, GetNetworkError> {
        match network_selection {
            NetworkSelection::Named(network_name) => {
                if self.project.exists().await? {
                    let p = self.project.load().await?;
                    let net = p.networks.get(network_name).context(NetworkNotFoundSnafu {
                        name: network_name.to_owned(),
                    })?;
                    Ok(net.clone())
                } else if network_name == DEFAULT_MAINNET_NETWORK_NAME {
                    Ok(crate::Network {
                        name: DEFAULT_MAINNET_NETWORK_NAME.to_string(),
                        configuration: crate::network::Configuration::Connected {
                            connected: crate::network::Connected {
                                url: DEFAULT_MAINNET_NETWORK_URL.to_string(),
                                root_key: None,
                            },
                        },
                    })
                } else {
                    Err(GetNetworkError::NetworkNotFound {
                        name: network_name.to_owned(),
                    })
                }
            }
            NetworkSelection::Default => Err(GetNetworkError::DefaultNetwork),
            NetworkSelection::Url(url) => Ok(crate::Network {
                name: url.to_string(),
                configuration: crate::network::Configuration::Connected {
                    connected: crate::network::Connected {
                        url: url.to_string(),
                        root_key: None,
                    },
                },
            }),
        }
    }

    pub async fn get_canister_and_path_for_env(
        &self,
        canister_name: &str,
        environment: &EnvironmentSelection,
    ) -> Result<(PathBuf, Canister), GetEnvCanisterError> {
        let p = self.project.load().await?;
        let Some((path, canister)) = p.get_canister(canister_name) else {
            return CanisterNotFoundInProjectSnafu {
                canister_name: canister_name.to_owned(),
            }
            .fail();
        };

        let env = self.get_environment(environment).await?;
        if !env.contains_canister(canister_name) {
            return CanisterNotInEnvSnafu {
                canister_name: canister_name.to_owned(),
                environment_name: environment.name().to_owned(),
            }
            .fail();
        }
        Ok((path.clone(), canister.clone()))
    }

    /// Gets the canister ID for a given canister selection in a specified environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the environment cannot be loaded or if the canister ID cannot be found.
    pub async fn get_canister_id_for_env(
        &self,
        canister: &CanisterSelection,
        environment: &EnvironmentSelection,
    ) -> Result<Principal, GetCanisterIdForEnvError> {
        let principal = match canister {
            CanisterSelection::Named(canister_name) => {
                let env = self.get_environment(environment).await?;
                let is_cache = match env.network.configuration {
                    NetworkConfiguration::Managed { .. } => true,
                    NetworkConfiguration::Connected { .. } => false,
                };

                if !env.canisters.contains_key(canister_name) {
                    return CanisterNotFoundInEnvSnafu {
                        canister_name: canister_name.to_owned(),
                        environment_name: environment.name().to_owned(),
                    }
                    .fail();
                }

                // Lookup the canister id
                self.ids
                    .lookup(is_cache, &env.name, canister_name)
                    .context(CanisterIdLookupSnafu {
                        canister_name: canister_name.to_owned(),
                        environment_name: environment.name().to_owned(),
                    })?
            }
            CanisterSelection::Principal(principal) => {
                // Make sure a valid environment was requested
                let _ = self.get_environment(environment).await?;
                *principal
            }
        };

        Ok(principal)
    }

    /// Sets the canister ID for a given canister name in a specified environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the environment cannot be loaded or if the canister ID cannot be registered.
    pub async fn set_canister_id_for_env(
        &self,
        canister_name: &str,
        canister_id: Principal,
        environment: &EnvironmentSelection,
    ) -> Result<(), SetCanisterIdForEnvError> {
        let env = self.get_environment(environment).await?;
        let is_cache = match env.network.configuration {
            NetworkConfiguration::Managed { .. } => true,
            NetworkConfiguration::Connected { .. } => false,
        };

        if !env.canisters.contains_key(canister_name) {
            return SetCanisterNotFoundInEnvSnafu {
                canister_name: canister_name.to_owned(),
                environment_name: environment.name().to_owned(),
            }
            .fail();
        }

        // Register the canister id
        self.ids
            .register(is_cache, &env.name, canister_name, canister_id)
            .context(CanisterIdRegisterSnafu {
                canister_name: canister_name.to_owned(),
                environment_name: environment.name().to_owned(),
            })?;

        Ok(())
    }

    /// Removes the canister ID for a given canister name in a specified environment.
    pub async fn remove_canister_id_for_env(
        &self,
        canister_name: &str,
        environment: &EnvironmentSelection,
    ) -> Result<(), RemoveCanisterIdForEnvError> {
        let env = self.get_environment(environment).await?;
        let is_cache = match env.network.configuration {
            NetworkConfiguration::Managed { .. } => true,
            NetworkConfiguration::Connected { .. } => false,
        };

        // Unregister the canister id
        self.ids
            .unregister(is_cache, &env.name, canister_name)
            .context(CanisterIdUnregisterSnafu {
                canister_name: canister_name.to_owned(),
                environment_name: environment.name().to_owned(),
            })?;

        Ok(())
    }

    /// Creates an agent for a given identity and environment.
    pub async fn get_agent_for_env(
        &self,
        identity: &IdentitySelection,
        environment: &EnvironmentSelection,
    ) -> Result<Agent, GetAgentForEnvError> {
        let id = self.get_identity(identity).await?;
        let env = self.get_environment(environment).await?;
        let access = self.network.access(&env.network).await?;
        Ok(self.create_agent(id, access).await?)
    }

    /// Creates an agent for a given identity and network.
    pub async fn get_agent_for_network(
        &self,
        identity: &IdentitySelection,
        network_selection: &NetworkSelection,
    ) -> Result<Agent, GetAgentForNetworkError> {
        let id = self.get_identity(identity).await?;
        let network = self.get_network(network_selection).await?;
        let access = self.network.access(&network).await?;
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
        let agent = self.agent.create(id, network_access.url.as_str()).await?;
        if let Some(k) = network_access.root_key {
            agent.set_root_key(k);
        }
        Ok(agent)
    }

    /// Creates an agent for a given identity and url.
    pub async fn get_agent_for_url(
        &self,
        identity: &IdentitySelection,
        url: &Url,
    ) -> Result<Agent, GetAgentForUrlError> {
        let id = self.get_identity(identity).await?;
        let agent = self.agent.create(id, url.as_str()).await?;
        Ok(agent)
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
            | (EnvironmentSelection::Named(_), NetworkSelection::Url(_)) => {
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
                                source: crate::ProjectLoadError::Locate { .. },
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
            | (EnvironmentSelection::Default, NetworkSelection::Url(_)) => {
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
                    | (EnvironmentSelection::Named(_), NetworkSelection::Url(_)) => {
                        Err(GetCanisterIdError::CanisterEnvironmentAndNetworkSpecified)
                    }

                    // Error: Canister by name with explicit network but no environment
                    (EnvironmentSelection::Default, NetworkSelection::Named(_))
                    | (EnvironmentSelection::Default, NetworkSelection::Url(_)) => {
                        Err(GetCanisterIdError::AmbiguousCanisterName)
                    }

                    // Only environment specified
                    (_, NetworkSelection::Default) => {
                        Ok(self.get_canister_id_for_env(canister, environment).await?)
                    }
                }
            }
        }
    }

    pub async fn ids_by_environment(
        &self,
        environment: &EnvironmentSelection,
    ) -> Result<IdMapping, GetIdsByEnvironmentError> {
        let env = self.get_environment(environment).await?;
        let is_cache = match env.network.configuration {
            NetworkConfiguration::Managed { .. } => true,
            NetworkConfiguration::Connected { .. } => false,
        };
        self.ids
            .lookup_by_environment(is_cache, environment.name())
            .context(IdsByEnvironmentLookupSnafu {
                environment_name: environment.name().to_owned(),
            })
    }

    #[cfg(test)]
    /// Creates a test context with all mocks
    pub fn mocked() -> Context {
        Context {
            term: Term::stderr(),
            dirs: Arc::new(crate::directories::UnimplementedMockDirs),
            ids: Arc::new(crate::store_id::mock::MockInMemoryIdStore::new()),
            artifacts: Arc::new(crate::store_artifact::MockInMemoryArtifactStore::new()),
            project: Arc::new(crate::MockProjectLoader::minimal()),
            identity: Arc::new(crate::identity::MockIdentityLoader::anonymous()),
            network: Arc::new(crate::network::MockNetworkAccessor::new()),
            agent: Arc::new(crate::agent::Creator),
            builder: Arc::new(crate::canister::build::UnimplementedMockBuilder),
            syncer: Arc::new(crate::canister::sync::UnimplementedMockSyncer),
            debug: false,
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
pub enum GetEnvironmentError {
    #[snafu(transparent)]
    ProjectLoad { source: crate::ProjectLoadError },

    #[snafu(display("environment '{}' not found in project", name))]
    EnvironmentNotFound { name: String },
}

#[derive(Debug, Snafu)]
pub enum GetNetworkError {
    #[snafu(transparent)]
    ProjectLoad { source: crate::ProjectLoadError },

    #[snafu(display("network '{}' not found in project", name))]
    NetworkNotFound { name: String },

    #[snafu(display("cannot load URL-specified network"))]
    UrlSpecifiedNetwork,

    #[snafu(display("cannot load default network"))]
    DefaultNetwork,
}

#[derive(Debug, Snafu)]
pub enum GetCanisterIdForEnvError {
    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(display(
        "canister '{}' not found in environment '{}'",
        canister_name,
        environment_name
    ))]
    CanisterNotFoundInEnv {
        canister_name: String,
        environment_name: String,
    },

    #[snafu(display(
        "failed to lookup canister ID for canister '{}' in environment '{}'",
        canister_name,
        environment_name
    ))]
    CanisterIdLookup {
        #[snafu(source(from(LookupIdError, Box::new)))]
        source: Box<LookupIdError>,
        canister_name: String,
        environment_name: String,
    },
}

#[derive(Debug, Snafu)]
pub enum SetCanisterIdForEnvError {
    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(display(
        "canister '{}' not found in environment '{}'",
        canister_name,
        environment_name
    ))]
    SetCanisterNotFoundInEnv {
        canister_name: String,
        environment_name: String,
    },

    #[snafu(display(
        "failed to register canister ID for canister '{}' in environment '{}'",
        canister_name,
        environment_name
    ))]
    CanisterIdRegister {
        source: crate::store_id::RegisterError,
        canister_name: String,
        environment_name: String,
    },
}

#[derive(Debug, Snafu)]
pub enum RemoveCanisterIdForEnvError {
    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(display(
        "failed to unregister canister ID for canister '{}' in environment '{}': {}",
        canister_name,
        environment_name,
        source
    ))]
    CanisterIdUnregister {
        source: crate::store_id::UnregisterError,
        canister_name: String,
        environment_name: String,
    },
}

#[derive(Debug, Snafu)]
pub enum GetAgentForEnvError {
    #[snafu(transparent)]
    GetIdentity { source: GetIdentityError },

    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(transparent)]
    NetworkAccess { source: crate::network::AccessError },

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
    NetworkAccess { source: crate::network::AccessError },

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
pub enum GetAgentError {
    #[snafu(transparent)]
    ProjectExists { source: crate::ProjectLoadError },

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

#[derive(Debug, Snafu)]
pub enum GetIdsByEnvironmentError {
    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(display("failed to lookup IDs for environment '{environment_name}'"))]
    IdsByEnvironmentLookup {
        source: crate::store_id::LookupIdError,
        environment_name: String,
    },
}

#[derive(Debug, Snafu)]
pub enum GetEnvCanisterError {
    #[snafu(transparent)]
    ProjectLoad { source: crate::ProjectLoadError },

    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(display("project does not contain a canister named '{canister_name}'"))]
    CanisterNotFoundInProject { canister_name: String },

    #[snafu(display(
        "environment '{environment_name}' does not contain a canister named '{canister_name}'"
    ))]
    CanisterNotInEnv {
        canister_name: String,
        environment_name: String,
    },
}

#[cfg(test)]
mod tests;
