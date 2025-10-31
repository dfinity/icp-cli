use std::sync::Arc;

use candid::Principal;
use clap::Subcommand;
use console::Term;
use ic_agent::{Agent, Identity};
use icp::{
    canister::{build::Build, sync::Synchronize},
    identity::IdentitySelection,
    network::access::NetworkAccess,
};
use snafu::{OptionExt, ResultExt, Snafu};

use crate::store_id::Key;

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
    pub(crate) dirs: Arc<dyn icp::directories::Access>,

    /// Canisters ID Store for lookup and storage
    pub(crate) ids: Arc<dyn crate::store_id::Access>,

    /// An artifact store for canister build artifacts
    pub(crate) artifacts: Arc<dyn crate::store_artifact::Access>,

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
            .context(IdentityLoadSnafu {
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
        let p = self.project.load().await?;

        // Load target environment
        let env = p
            .environments
            .get(environment_name)
            .context(EnvironmentNotFoundSnafu {
                name: environment_name.to_owned(),
            })?;

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
    pub(crate) async fn get_canister_id_for_env(
        &self,
        canister_name: &str,
        environment_name: &str,
    ) -> Result<Principal, GetCanisterIdForEnvError> {
        let env = self.get_environment(environment_name).await?;

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
            .context(CanisterIdLookupSnafu {
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
        let id = self.get_identity(identity).await?;
        let env = self.get_environment(environment_name).await?;
        let access = self.network.access(&env.network).await?;
        Ok(self.create_agent(id, access).await?)
    }

    /// Creates an agent for a given identity and network.
    pub(crate) async fn get_agent_for_network(
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
        let id = self.get_identity(identity).await?;
        let agent = self.agent.create(id, url).await?;
        Ok(agent)
    }

    #[cfg(test)]
    /// Creates a test context with all mocks
    pub(crate) fn mocked() -> Context {
        Context {
            term: Term::stderr(),
            dirs: Arc::new(icp::directories::UnimplementedMockDirs),
            ids: Arc::new(crate::store_id::MockInMemoryIdStore::new()),
            artifacts: Arc::new(crate::store_artifact::MockInMemoryArtifactStore::new()),
            project: Arc::new(icp::MockProjectLoader::minimal()),
            identity: Arc::new(icp::identity::MockIdentityLoader::anonymous()),
            network: Arc::new(icp::network::MockNetworkAccessor::new()),
            agent: Arc::new(icp::agent::Creator),
            builder: Arc::new(icp::canister::build::UnimplementedMockBuilder),
            syncer: Arc::new(icp::canister::sync::UnimplementedMockSyncer),
            debug: false,
        }
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum GetIdentityError {
    #[snafu(display("failed to load identity"))]
    IdentityLoad {
        source: icp::identity::LoadError,
        identity: IdentitySelection,
    },
}

#[derive(Debug, Snafu)]
pub(crate) enum GetEnvironmentError {
    #[snafu(transparent)]
    ProjectLoad { source: icp::LoadError },

    #[snafu(display("environment '{}' not found in project", name))]
    EnvironmentNotFound { name: String },
}

#[derive(Debug, Snafu)]
pub(crate) enum GetNetworkError {
    #[snafu(transparent)]
    ProjectLoad { source: icp::LoadError },

    #[snafu(display("network '{}' not found in project", name))]
    NetworkNotFound { name: String },
}

#[derive(Debug, Snafu)]
pub(crate) enum GetCanisterIdForEnvError {
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
pub(crate) enum GetAgentForEnvError {
    #[snafu(transparent)]
    GetIdentity { source: GetIdentityError },

    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(transparent)]
    NetworkAccess { source: icp::network::AccessError },

    #[snafu(transparent)]
    AgentCreate { source: icp::agent::CreateError },
}

#[derive(Debug, Snafu)]
pub(crate) enum GetAgentForNetworkError {
    #[snafu(transparent)]
    GetIdentity { source: GetIdentityError },

    #[snafu(transparent)]
    GetNetwork { source: GetNetworkError },

    #[snafu(transparent)]
    NetworkAccess { source: icp::network::AccessError },

    #[snafu(transparent)]
    AgentCreate { source: icp::agent::CreateError },
}

#[derive(Debug, Snafu)]
pub(crate) enum GetAgentForUrlError {
    #[snafu(transparent)]
    GetIdentity { source: GetIdentityError },

    #[snafu(transparent)]
    AgentCreate { source: icp::agent::CreateError },
}

#[cfg(test)]
mod context_tests {
    use super::*;
    use crate::store_id::MockInMemoryIdStore;
    use icp::{MockProjectLoader, identity::MockIdentityLoader, network::MockNetworkAccessor};

    #[tokio::test]
    async fn test_get_identity_default() {
        let ctx = Context::mocked();

        let result = ctx.get_identity(&IdentitySelection::Default).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_identity_anonymous() {
        let ctx = Context::mocked();

        let result = ctx.get_identity(&IdentitySelection::Anonymous).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_identity_named() {
        let alice_identity: Arc<dyn Identity> = Arc::new(ic_agent::identity::AnonymousIdentity);

        let ctx = Context {
            identity: Arc::new(
                MockIdentityLoader::anonymous().with_identity("alice", Arc::clone(&alice_identity)),
            ),
            ..Context::mocked()
        };

        let result = ctx
            .get_identity(&IdentitySelection::Named("alice".to_string()))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_identity_named_not_found() {
        let ctx = Context::mocked();

        let result = ctx
            .get_identity(&IdentitySelection::Named("nonexistent".to_string()))
            .await;

        assert!(matches!(
            result,
            Err(GetIdentityError::IdentityLoad {
                identity: IdentitySelection::Named(_),
                source: icp::identity::LoadError::LoadIdentity(_)
            })
        ));
    }

    #[tokio::test]
    async fn test_get_environment_success() {
        let ctx = Context {
            project: Arc::new(MockProjectLoader::complex()),
            ..Context::mocked()
        };

        let env = ctx.get_environment("dev").await.unwrap();

        assert_eq!(env.name, "dev");
    }

    #[tokio::test]
    async fn test_get_environment_not_found() {
        let ctx = Context::mocked();

        let result = ctx.get_environment("nonexistent").await;

        assert!(matches!(
            result,
            Err(GetEnvironmentError::EnvironmentNotFound { ref name }) if name == "nonexistent"
        ));
    }

    #[tokio::test]
    async fn test_get_network_success() {
        let ctx = Context {
            project: Arc::new(MockProjectLoader::complex()),
            ..Context::mocked()
        };

        let network = ctx.get_network("local").await.unwrap();

        assert_eq!(network.name, "local");
    }

    #[tokio::test]
    async fn test_get_network_not_found() {
        let ctx = Context::mocked();

        let result = ctx.get_network("nonexistent").await;

        assert!(matches!(
            result,
            Err(GetNetworkError::NetworkNotFound { ref name }) if name == "nonexistent"
        ));
    }

    #[tokio::test]
    async fn test_get_canister_id_for_env_success() {
        use crate::store_id::{Access as IdAccess, Key};
        use candid::Principal;

        let ids_store = Arc::new(MockInMemoryIdStore::new());

        // Register a canister ID for the dev environment
        let canister_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        ids_store
            .register(
                &Key {
                    network: "local".to_string(),
                    environment: "dev".to_string(),
                    canister: "backend".to_string(),
                },
                &canister_id,
            )
            .unwrap();

        let ctx = Context {
            project: Arc::new(MockProjectLoader::complex()),
            ids: ids_store,
            ..Context::mocked()
        };

        let cid = ctx.get_canister_id_for_env("backend", "dev").await.unwrap();

        assert_eq!(cid, canister_id);
    }

    #[tokio::test]
    async fn test_get_canister_id_for_env_canister_not_in_env() {
        let ctx = Context {
            project: Arc::new(MockProjectLoader::complex()),
            ..Context::mocked()
        };

        // "database" is only in "dev" environment, not in "test"
        let result = ctx.get_canister_id_for_env("database", "test").await;

        assert!(matches!(
            result,
            Err(GetCanisterIdForEnvError::CanisterNotFoundInEnv {
                ref canister_name,
                ref environment_name,
            }) if canister_name == "database" && environment_name == "test"
        ));
    }

    #[tokio::test]
    async fn test_get_canister_id_for_env_id_not_registered() {
        let ctx = Context {
            project: Arc::new(MockProjectLoader::complex()),
            ..Context::mocked()
        };

        // Environment exists and canister is in it, but ID not registered
        let result = ctx.get_canister_id_for_env("backend", "dev").await;

        assert!(matches!(
            result,
            Err(GetCanisterIdForEnvError::CanisterIdLookup {
                ref canister_name,
                ref environment_name,
                ..
            }) if canister_name == "backend" && environment_name == "dev"
        ));
    }

    #[tokio::test]
    async fn test_get_agent_for_env_uses_environment_network() {
        use icp::network::access::NetworkAccess;

        let staging_root_key = vec![1, 2, 3];

        // Complex project has "test" environment which uses "staging" network
        let ctx = Context {
            project: Arc::new(MockProjectLoader::complex()),
            network: Arc::new(
                MockNetworkAccessor::new()
                    .with_network(
                        "local",
                        NetworkAccess {
                            default_effective_canister_id: None,
                            root_key: None,
                            url: "http://localhost:8000".to_string(),
                        },
                    )
                    .with_network(
                        "staging",
                        NetworkAccess {
                            default_effective_canister_id: None,
                            root_key: Some(staging_root_key.clone()),
                            url: "http://staging:9000".to_string(),
                        },
                    ),
            ),
            ..Context::mocked()
        };

        let agent = ctx
            .get_agent_for_env(&IdentitySelection::Anonymous, "test")
            .await
            .unwrap();

        assert_eq!(agent.read_root_key(), staging_root_key);
    }

    #[tokio::test]
    async fn test_get_agent_for_env_environment_not_found() {
        let ctx = Context::mocked();

        let result = ctx
            .get_agent_for_env(&IdentitySelection::Anonymous, "nonexistent")
            .await;

        assert!(matches!(
            result,
            Err(GetAgentForEnvError::GetEnvironment {
                source: GetEnvironmentError::EnvironmentNotFound { .. }
            })
        ));
    }

    #[tokio::test]
    async fn test_get_agent_for_env_network_not_configured() {
        // Environment "dev" exists in project and uses "local" network,
        // but "local" network is not configured in MockNetworkAccessor
        let ctx = Context {
            project: Arc::new(MockProjectLoader::complex()),
            // MockNetworkAccessor has no networks configured
            ..Context::mocked()
        };

        let result = ctx
            .get_agent_for_env(&IdentitySelection::Anonymous, "dev")
            .await;

        assert!(matches!(
            result,
            Err(GetAgentForEnvError::NetworkAccess {
                source: icp::network::AccessError::Unexpected(_)
            })
        ));
    }

    #[tokio::test]
    async fn test_get_agent_for_network_success() {
        use icp::network::access::NetworkAccess;

        let root_key = vec![1, 2, 3];

        let ctx = Context {
            project: Arc::new(MockProjectLoader::complex()),
            network: Arc::new(MockNetworkAccessor::new().with_network(
                "local",
                NetworkAccess {
                    default_effective_canister_id: None,
                    root_key: Some(root_key.clone()),
                    url: "http://localhost:8000".to_string(),
                },
            )),
            ..Context::mocked()
        };

        let agent = ctx
            .get_agent_for_network(&IdentitySelection::Anonymous, "local")
            .await
            .unwrap();

        assert_eq!(agent.read_root_key(), root_key);
    }

    #[tokio::test]
    async fn test_get_agent_for_network_network_not_found() {
        let ctx = Context::mocked();

        let result = ctx
            .get_agent_for_network(&IdentitySelection::Anonymous, "nonexistent")
            .await;

        assert!(matches!(
            result,
            Err(GetAgentForNetworkError::GetNetwork {
                source: GetNetworkError::NetworkNotFound { .. }
            })
        ));
    }

    #[tokio::test]
    async fn test_get_agent_for_network_not_configured() {
        // Network "local" exists in project but is not configured in MockNetworkAccessor
        let ctx = Context {
            project: Arc::new(MockProjectLoader::complex()),
            // MockNetworkAccessor has no networks configured
            ..Context::mocked()
        };

        let result = ctx
            .get_agent_for_network(&IdentitySelection::Anonymous, "local")
            .await;

        assert!(matches!(
            result,
            Err(GetAgentForNetworkError::NetworkAccess {
                source: icp::network::AccessError::Unexpected(_)
            })
        ));
    }

    #[tokio::test]
    async fn test_get_agent_for_url_success() {
        let ctx = Context::mocked();

        let result = ctx
            .get_agent_for_url(&IdentitySelection::Anonymous, "http://localhost:8000")
            .await;

        assert!(result.is_ok());
    }
}
