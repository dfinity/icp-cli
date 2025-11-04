use console::Term;
use std::sync::Arc;
use url::Url;

use crate::{
    agent::CreateAgentError,
    canister::{build::Build, sync::Synchronize},
    directories,
    identity::IdentitySelection,
    network::access::NetworkAccess,
};
use candid::Principal;
use ic_agent::{Agent, Identity};
use snafu::{OptionExt, ResultExt, Snafu};

mod init;

use crate::store_id::Key;

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
            EnvironmentSelection::Default => "local",
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
    pub project: Arc<dyn crate::Load>,

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
    pub async fn get_network(&self, network_name: &str) -> Result<crate::Network, GetNetworkError> {
        // Load project
        let p = self.project.load().await?;

        // Load target network
        let net = p.networks.get(network_name).context(NetworkNotFoundSnafu {
            name: network_name.to_owned(),
        })?;

        Ok(net.clone())
    }

    /// Gets the canister ID for a given canister name in a specified environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the environment cannot be loaded or if the canister ID cannot be found.
    pub async fn get_canister_id_for_env(
        &self,
        canister_name: &str,
        environment: &EnvironmentSelection,
    ) -> Result<Principal, GetCanisterIdForEnvError> {
        let env = self.get_environment(environment).await?;

        if !env.canisters.contains_key(canister_name) {
            return Err(GetCanisterIdForEnvError::CanisterNotFoundInEnv {
                canister_name: canister_name.to_owned(),
                environment_name: environment.name().to_owned(),
            });
        }

        // Lookup the canister id
        let cid = self
            .ids
            .lookup(&Key {
                environment: env.name.to_owned(),
                canister: canister_name.to_owned(),
            })
            .context(CanisterIdLookupSnafu {
                canister_name: canister_name.to_owned(),
                environment_name: environment.name().to_owned(),
            })?;

        Ok(cid)
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
        network_name: &str,
    ) -> Result<Agent, GetAgentForNetworkError> {
        let id = self.get_identity(identity).await?;
        let network = self.get_network(network_name).await?;
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

    /// Gets a canister ID for a given canister and environment selection.
    ///
    /// This method validates that the environment exists when using a principal,
    /// or looks up the canister ID when using a name.
    pub async fn get_canister_id(
        &self,
        canister: &CanisterSelection,
        environment: &EnvironmentSelection,
    ) -> Result<Principal, GetCanisterIdError> {
        let principal = match canister {
            CanisterSelection::Named(canister_name) => {
                self.get_canister_id_for_env(canister_name, environment)
                    .await?
            }
            CanisterSelection::Principal(principal) => {
                // Make sure a valid environment was requested
                let _ = self.get_environment(environment).await?;
                *principal
            }
        };

        Ok(principal)
    }

    /// Gets a canister ID and agent for the given selections.
    ///
    /// This is the main entry point for commands that need to interact with a canister.
    /// It handles all the different combinations of canister, environment, and network selections.
    pub async fn get_canister_id_and_agent(
        &self,
        canister: &CanisterSelection,
        environment: &EnvironmentSelection,
        network: &NetworkSelection,
        identity: &IdentitySelection,
    ) -> Result<(Principal, Agent), GetCanisterIdAndAgentError> {
        let (cid, agent) = match (canister, environment, network) {
            // Error: Both environment and network specified
            (_, EnvironmentSelection::Named(_), NetworkSelection::Named(_))
            | (_, EnvironmentSelection::Named(_), NetworkSelection::Url(_)) => {
                return Err(GetCanisterIdAndAgentError::EnvironmentAndNetworkSpecified);
            }

            // Error: Canister by name with default environment and explicit network
            (
                CanisterSelection::Named(_),
                EnvironmentSelection::Default,
                NetworkSelection::Named(_),
            )
            | (
                CanisterSelection::Named(_),
                EnvironmentSelection::Default,
                NetworkSelection::Url(_),
            ) => {
                return Err(GetCanisterIdAndAgentError::AmbiguousCanisterName);
            }

            // Canister by name, use environment
            (CanisterSelection::Named(cname), _, NetworkSelection::Default) => {
                let agent = self.get_agent_for_env(identity, environment).await?;
                let cid = self.get_canister_id_for_env(cname, environment).await?;
                (cid, agent)
            }

            // Canister by principal, use environment
            (CanisterSelection::Principal(principal), _, NetworkSelection::Default) => {
                let agent = self.get_agent_for_env(identity, environment).await?;
                (*principal, agent)
            }

            // Canister by principal, use named network (environment must be default)
            (
                CanisterSelection::Principal(principal),
                EnvironmentSelection::Default,
                NetworkSelection::Named(net_name),
            ) => {
                let agent = self.get_agent_for_network(identity, net_name).await?;
                (*principal, agent)
            }

            // Canister by principal, use URL network (environment must be default)
            (
                CanisterSelection::Principal(principal),
                EnvironmentSelection::Default,
                NetworkSelection::Url(url),
            ) => {
                let agent = self.get_agent_for_url(identity, url).await?;
                (*principal, agent)
            }
        };

        Ok((cid, agent))
    }

    #[cfg(test)]
    /// Creates a test context with all mocks
    pub fn mocked() -> Context {
        Context {
            term: Term::stderr(),
            dirs: Arc::new(crate::directories::UnimplementedMockDirs),
            ids: Arc::new(crate::store_id::MockInMemoryIdStore::new()),
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
    ProjectLoad { source: crate::LoadError },

    #[snafu(display("environment '{}' not found in project", name))]
    EnvironmentNotFound { name: String },
}

#[derive(Debug, Snafu)]
pub enum GetNetworkError {
    #[snafu(transparent)]
    ProjectLoad { source: crate::LoadError },

    #[snafu(display("network '{}' not found in project", name))]
    NetworkNotFound { name: String },
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
        source: crate::store_id::LookupIdError,
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
pub enum GetCanisterIdError {
    #[snafu(transparent)]
    GetCanisterIdForEnv { source: GetCanisterIdForEnvError },

    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },
}

#[derive(Debug, Snafu)]
pub enum GetCanisterIdAndAgentError {
    #[snafu(display("You can't specify both an environment and a network"))]
    EnvironmentAndNetworkSpecified,

    #[snafu(display(
        "Specifying a network is not supported if you are targeting a canister by name, specify an environment instead"
    ))]
    AmbiguousCanisterName,

    #[snafu(transparent)]
    GetCanisterIdForEnv { source: GetCanisterIdForEnvError },

    #[snafu(transparent)]
    GetAgentForEnv { source: GetAgentForEnvError },

    #[snafu(transparent)]
    GetAgentForNetwork { source: GetAgentForNetworkError },

    #[snafu(transparent)]
    GetAgentForUrl { source: GetAgentForUrlError },
}

#[cfg(test)]
mod tests;
