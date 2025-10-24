use std::sync::Arc;

use candid::Principal;
use clap::Subcommand;
use console::Term;
use ic_agent::{Agent, Identity};
use icp::{
    Directories,
    canister::{build::Build, sync::Synchronize},
    identity::IdentitySelection,
    network::access::NetworkAccess,
};
use snafu::{OptionExt, ResultExt, Snafu};

use crate::store_id::Key;
use crate::{store_artifact::ArtifactStore, store_id::IdStore};

pub(crate) mod args;
pub(crate) mod build;
pub(crate) mod canister;
pub(crate) mod cycles;
pub(crate) mod deploy;
pub(crate) mod environment;
pub(crate) mod identity;
pub(crate) mod network;
pub(crate) mod project;
pub(crate) mod sync;
pub(crate) mod token;

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Command {
    /// Build a project
    Build(build::BuildArgs),

    /// Perform canister operations against a network
    #[command(subcommand)]
    Canister(canister::Command),

    /// Mint and manage cycles
    #[command(subcommand)]
    Cycles(cycles::Command),

    /// Deploy a project to an environment
    Deploy(deploy::DeployArgs),

    /// Show information about the current project environments
    #[command(subcommand)]
    Environment(environment::Command),

    /// Manage your identities
    #[command(subcommand)]
    Identity(identity::Command),

    /// Launch and manage local test networks
    #[command(subcommand)]
    Network(network::Command),

    /// Display information about the current project
    #[clap(hide = true)] // TODO: figure out how to structure the commands later
    #[command(subcommand)]
    Project(project::Command),

    /// Synchronize canisters in the current environment
    Sync(sync::SyncArgs),

    /// Perform token transactions
    Token(token::Command),
}

pub(crate) struct Context {
    /// Terminal for printing messages for the user to see
    pub(crate) term: Term,

    /// Various cli-related directories (cache, configuration, etc).
    pub(crate) dirs: Directories,

    /// Canisters ID Store for lookup and storage
    pub(crate) ids: IdStore,

    /// An artifact store for canister build artifacts
    pub(crate) artifacts: ArtifactStore,

    /// Project loader
    pub(crate) project: Arc<dyn icp::Load>,

    /// Identity loader
    pub(crate) identity: Arc<dyn icp::identity::Load>,

    /// NetworkAccess loader
    pub(crate) network: Arc<dyn icp::network::Access>,

    /// Agent creator
    pub(crate) agent: Arc<dyn icp::agent::Create>,

    /// Canister builder
    pub(crate) builder: Arc<dyn Build>,

    /// Canister synchronizer
    pub(crate) syncer: Arc<dyn Synchronize>,

    /// Whether debug is enabled
    pub(crate) debug: bool,
}

impl Context {
    /// Gets an identity based on the provided identity selection.
    // TODO: refactor the whole codebase to use this method instead of directly accessing `ctx.identity.load()`
    pub(crate) async fn get_identity(
        &self,
        identity: &IdentitySelection,
    ) -> Result<Arc<dyn Identity>, GetIdentityError> {
        self.identity
            .load(identity.clone())
            .await
            .context(get_identity_error::IdentityLoadSnafu {
                identity: identity.clone(),
            })
    }

    /// Gets an environment by name from the currently loaded project.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be loaded or if the environment is not found.
    pub(crate) async fn get_environment(
        &self,
        environment_name: &str,
    ) -> Result<icp::Environment, GetEnvironmentError> {
        // Load project
        let p = self
            .project
            .load()
            .await
            .context(get_environment_error::ProjectLoadSnafu)?;

        // Load target environment
        let env = p.environments.get(environment_name).context(
            get_environment_error::EnvironmentNotFoundSnafu {
                name: environment_name.to_owned(),
            },
        )?;

        Ok(env.clone())
    }

    /// Gets an Network by name from the currently loaded project.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be loaded or if the network is not found.
    pub(crate) async fn get_network(
        &self,
        network_name: &str,
    ) -> Result<icp::Network, GetNetworkError> {
        // Load project
        let p = self
            .project
            .load()
            .await
            .context(get_network_error::ProjectLoadSnafu)?;

        // Load target network
        let net =
            p.networks
                .get(network_name)
                .context(get_network_error::NetworkNotFoundSnafu {
                    name: network_name.to_owned(),
                })?;

        Ok(net.clone())
    }

    /// Gets the canister ID for a given canister name in a specified environment.
    ///
    /// # Errors
    ///
    /// Returns an error if the environment cannot be loaded or if the canister ID cannot be found.
    pub(crate) async fn get_canister_id_for_env(
        &self,
        canister_name: &str,
        environment_name: &str,
    ) -> Result<Principal, GetCanisterIdForEnvError> {
        let env = self.get_environment(environment_name).await.context(
            get_canister_id_for_env_error::GetEnvironmentSnafu {
                environment_name: environment_name.to_owned(),
            },
        )?;

        if !env.canisters.contains_key(canister_name) {
            return Err(GetCanisterIdForEnvError::CanisterNotFoundInEnv {
                canister_name: canister_name.to_owned(),
                environment_name: environment_name.to_owned(),
            });
        }

        // Lookup the canister id
        let cid = self
            .ids
            .lookup(&Key {
                network: env.network.name.to_owned(),
                environment: env.name.to_owned(),
                canister: canister_name.to_owned(),
            })
            .context(get_canister_id_for_env_error::CanisterIdLookupSnafu {
                canister_name: canister_name.to_owned(),
                environment_name: environment_name.to_owned(),
            })?;

        Ok(cid)
    }

    /// Creates an agent for a given identity and environment.
    pub(crate) async fn get_agent_for_env(
        &self,
        identity: &IdentitySelection,
        environment_name: &str,
    ) -> Result<Agent, GetAgentForEnvError> {
        let id = self.get_identity(identity).await.context(
            get_agent_for_env_error::GetIdentitySnafu {
                identity: identity.to_owned(),
            },
        )?;
        let env = self.get_environment(environment_name).await.context(
            get_agent_for_env_error::GetEnvironmentSnafu {
                environment_name: environment_name.to_owned(),
            },
        )?;
        let access = self.network.access(&env.network).await.context(
            get_agent_for_env_error::NetworkAccessSnafu {
                network_name: env.network.name.to_owned(),
            },
        )?;
        self.create_agent(id, access)
            .await
            .context(get_agent_for_env_error::AgentCreateSnafu)
    }

    /// Creates an agent for a given identity and network.
    pub(crate) async fn get_agent_for_network(
        &self,
        identity: &IdentitySelection,
        network_name: &str,
    ) -> Result<Agent, GetAgentForNetworkError> {
        let id = self.get_identity(identity).await.context(
            get_agent_for_network_error::GetIdentitySnafu {
                identity: identity.to_owned(),
            },
        )?;
        let network = self.get_network(network_name).await.context(
            get_agent_for_network_error::GetNetworkSnafu {
                network_name: network_name.to_owned(),
            },
        )?;
        let access = self.network.access(&network).await.context(
            get_agent_for_network_error::NetworkAccessSnafu {
                network_name: network_name.to_owned(),
            },
        )?;
        self.create_agent(id, access)
            .await
            .context(get_agent_for_network_error::AgentCreateSnafu)
    }

    /// Private helper to create an agent given identity and network access.
    ///
    /// Used by [`Self::get_agent_for_env`] and [`Self::get_agent_for_network`].
    async fn create_agent(
        &self,
        id: Arc<dyn Identity>,
        network_access: NetworkAccess,
    ) -> Result<Agent, icp::agent::CreateError> {
        let agent = self.agent.create(id, &network_access.url).await?;
        if let Some(k) = network_access.root_key {
            agent.set_root_key(k);
        }
        Ok(agent)
    }

    /// Creates an agent for a given identity and url.
    pub(crate) async fn get_agent_for_url(
        &self,
        identity: &IdentitySelection,
        url: &str, // TODO: change to Url
    ) -> Result<Agent, GetAgentForUrlError> {
        let id = self.get_identity(identity).await.context(
            get_agent_for_url_error::GetIdentitySnafu {
                identity: identity.to_owned(),
            },
        )?;
        let agent = self
            .agent
            .create(id, url)
            .await
            .context(get_agent_for_url_error::AgentCreateSnafu)?;
        Ok(agent)
    }
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub(crate) enum GetIdentityError {
    #[snafu(display("failed to load identity"))]
    IdentityLoad {
        source: icp::identity::LoadError,
        identity: IdentitySelection,
    },
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub(crate) enum GetEnvironmentError {
    #[snafu(display("failed to load project"))]
    ProjectLoad { source: icp::LoadError },

    #[snafu(display("environment '{}' not found in project", name))]
    EnvironmentNotFound { name: String },
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub(crate) enum GetNetworkError {
    #[snafu(display("failed to load project"))]
    ProjectLoad { source: icp::LoadError },

    #[snafu(display("network '{}' not found in project", name))]
    NetworkNotFound { name: String },
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub(crate) enum GetCanisterIdForEnvError {
    #[snafu(display("failed to get environment: {}", environment_name))]
    GetEnvironment {
        source: GetEnvironmentError,
        environment_name: String,
    },

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
        source: crate::store_id::LookupError,
        canister_name: String,
        environment_name: String,
    },
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub(crate) enum GetAgentForEnvError {
    #[snafu(display("failed to get identity"))]
    GetIdentity {
        source: GetIdentityError,
        identity: IdentitySelection,
    },

    #[snafu(display("failed to get environment '{}'", environment_name))]
    GetEnvironment {
        source: GetEnvironmentError,
        environment_name: String,
    },

    #[snafu(display("failed to access network: {}", network_name))]
    NetworkAccess {
        source: icp::network::AccessError,
        network_name: String,
    },

    #[snafu(display("failed to create agent: {}", source))]
    AgentCreate { source: icp::agent::CreateError },
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub(crate) enum GetAgentForNetworkError {
    #[snafu(display("failed to get identity"))]
    GetIdentity {
        source: GetIdentityError,
        identity: IdentitySelection,
    },

    #[snafu(display("failed to get network '{}'", network_name))]
    GetNetwork {
        source: GetNetworkError,
        network_name: String,
    },

    #[snafu(display("failed to access network: {}", network_name))]
    NetworkAccess {
        source: icp::network::AccessError,
        network_name: String,
    },

    #[snafu(display("failed to create agent: {}", source))]
    AgentCreate { source: icp::agent::CreateError },
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub(crate) enum GetAgentForUrlError {
    #[snafu(display("failed to get identity"))]
    GetIdentity {
        source: GetIdentityError,
        identity: IdentitySelection,
    },

    #[snafu(display("failed to create agent: {}", source))]
    AgentCreate { source: icp::agent::CreateError },
}
