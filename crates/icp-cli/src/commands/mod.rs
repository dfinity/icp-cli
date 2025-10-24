use std::sync::Arc;

use candid::Principal;
use clap::Subcommand;
use console::Term;
use ic_agent::Agent;
use icp::{
    Directories, Environment, Project,
    canister::{build::Build, sync::Synchronize},
};
use tokio::runtime::Handle;

use crate::{
    commands::args::ArgContext,
    store_artifact::ArtifactStore,
    store_id::{self, IdStore, Key},
};

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

#[derive(Debug, thiserror::Error)]
pub(crate) enum ContextError {
    #[error("Environment '{environment}' not found.")]
    EnvironmentNotFound { environment: String },

    #[error("Failed to load network info: {0}")]
    AccessNetwork(#[from] icp::network::AccessError),

    #[error("Failed to create agent: {0}")]
    CreateAgent(#[from] icp::agent::CreateError),

    #[error("Failed to load project: {0}")]
    LoadProject(#[from] icp::LoadError),

    #[error("Failed to load identity: {0}")]
    LoadIdentity(#[from] icp::identity::LoadError),

    #[error("Failed to lookup up canister id: {0}")]
    LookupCanisterId(#[from] store_id::LookupError),

    #[error("Failed to register canister id: {0}")]
    RegisterCanisterId(#[from] store_id::RegisterError),

    #[error("Network '{network}' does not contain canister '{canister}'")]
    NetworkCanisterNotFound { network: String, canister: String },

    #[error("Environment '{environment}' does not contain canister '{canister}'")]
    EnvironmentCanisterNotFound {
        environment: String,
        canister: String,
    },
}

pub(crate) struct Context {
    /// Terminal for printing messages for the user to see
    pub(crate) term: Term,

    /// Various cli-related directories (cache, configuration, etc).
    pub(crate) dirs: Directories,

    /// Canisters ID Store for lookup and storage
    pub(crate) ids: Arc<dyn IdStore>,

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
    pub(crate) fn get_project(&self) -> Result<Project, ContextError> {
        // Try to get the current runtime handle
        match Handle::try_current() {
            // Runtime exists, use it
            Ok(handle) => {
                let project = self.project.clone();
                handle.block_on(async move { project.load().await })
            }
            // No runtime, create one and block
            Err(_) => {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async { self.project.load().await })
            }
        }
        .map_err(ContextError::LoadProject)
    }

    pub(crate) fn get_environment(&self, args: &ArgContext) -> Result<Environment, ContextError> {
        let project = self.get_project()?;
        let environment = project.environments.get(args.environment()).ok_or(
            ContextError::EnvironmentNotFound {
                environment: args.environment().to_string(),
            },
        )?;
        Ok(environment.clone())
    }

    pub(crate) async fn get_agent(&self, args: &ArgContext) -> Result<Agent, ContextError> {
        let id = self.identity.load(args.identity().clone()).await?;
        let environment = self.get_environment(args)?;
        let access = self.network.access(&environment.network).await?;
        let agent = self.agent.create(id, &access.url).await?;
        if let Some(k) = access.root_key {
            agent.set_root_key(k);
        }
        Ok(agent)
    }

    pub(crate) fn resolve_canister_id(
        &self,
        args: &ArgContext,
        name: &str,
    ) -> Result<Principal, ContextError> {
        if let Ok(canister_id) = Principal::from_text(name) {
            return Ok(canister_id);
        }

        let environment = self.get_environment(args)?;
        let canister_id = self.ids.lookup(&Key {
            network: environment.network.name.to_owned(),
            environment: environment.name.to_owned(),
            canister: name.to_owned(),
        })?;
        Ok(canister_id)
    }

    pub(crate) fn store_canister_id(
        &self,
        args: &ArgContext,
        name: &str,
        canister_id: Principal,
    ) -> Result<(), ContextError> {
        let environment = self.get_environment(args)?;
        let key = Key {
            network: environment.network.name.to_owned(),
            environment: environment.name.to_owned(),
            canister: name.to_owned(),
        };
        self.ids.register(&key, &canister_id)?;
        Ok(())
    }

    pub(crate) fn ensure_canister_is_defined(
        &self,
        args: &ArgContext,
        name: &str,
    ) -> Result<(), ContextError> {
        let project = self.get_project()?;
        let environment = self.get_environment(args)?;
        if !project.contains_canister(name) {
            return Err(ContextError::NetworkCanisterNotFound {
                network: environment.network.name.to_owned(),
                canister: name.to_owned(),
            });
        }
        if !environment.contains_canister(name) {
            return Err(ContextError::EnvironmentCanisterNotFound {
                environment: environment.name.to_owned(),
                canister: name.to_owned(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod test_utils {
    use std::sync::Arc;

    use console::Term;
    use icp::{
        Directories,
        canister::{build::Build, sync::Synchronize},
        network::Access,
    };

    use super::Context;
    use crate::{
        store_artifact::ArtifactStore,
        store_id::{IdStore, test::MockIdStore},
    };

    /// Helper to create a test context with a default setup.
    ///
    /// Tests should not rely on actual filesystem I/O.
    pub(crate) fn create_test_context(project: icp::Project) -> Context {
        let fake_dir =
            icp::prelude::PathBuf::try_from(std::path::PathBuf::from("/fake/test-dir")).unwrap();
        let fake_artifacts =
            icp::prelude::PathBuf::try_from(std::path::PathBuf::from("/fake/artifacts")).unwrap();

        Context {
            term: Term::stdout(),
            dirs: Directories::Overridden(fake_dir),
            ids: Arc::new(MockIdStore::new()),
            artifacts: ArtifactStore::new(&fake_artifacts),
            project: Arc::new(icp::test::MockProjectLoader::new(project)),
            identity: Arc::new(icp::identity::test::MockIdentityLoader::new(
                icp::identity::test::create_mock_identity(),
            )),
            network: Arc::new(icp::network::test::MockNetworkAccessor::new()),
            agent: Arc::new(icp::agent::test::MockAgentCreator::default()),
            builder: Arc::new(icp::canister::build::test::MockBuilder::new()),
            syncer: Arc::new(icp::canister::sync::test::MockSynchronizer::new()),
            debug: false,
        }
    }

    // Helper to create a test context with specific mocks
    pub(crate) struct TestContextBuilder {
        project: Arc<dyn icp::Load>,
        identity: Arc<dyn icp::identity::Load>,
        network: Arc<dyn Access>,
        agent: Arc<dyn icp::agent::Create>,
        builder: Arc<dyn Build>,
        syncer: Arc<dyn Synchronize>,
        ids: Arc<dyn IdStore>,
        debug: bool,
    }

    impl TestContextBuilder {
        pub fn new() -> Self {
            Self {
                project: Arc::new(icp::test::MockProjectLoader::new(
                    icp::test::create_test_project(),
                )),
                identity: Arc::new(icp::identity::test::MockIdentityLoader::new(
                    icp::identity::test::create_mock_identity(),
                )),
                network: Arc::new(icp::network::test::MockNetworkAccessor::new()),
                agent: Arc::new(icp::agent::test::MockAgentCreator::default()),
                builder: Arc::new(icp::canister::build::test::MockBuilder::new()),
                syncer: Arc::new(icp::canister::sync::test::MockSynchronizer::new()),
                ids: Arc::new(MockIdStore::new()),
                debug: false,
            }
        }

        pub fn with_project(mut self, loader: Arc<dyn icp::Load>) -> Self {
            self.project = loader;
            self
        }

        pub fn with_identity(mut self, loader: Arc<dyn icp::identity::Load>) -> Self {
            self.identity = loader;
            self
        }

        pub fn with_network(mut self, accessor: Arc<dyn Access>) -> Self {
            self.network = accessor;
            self
        }

        pub fn with_agent(mut self, creator: Arc<dyn icp::agent::Create>) -> Self {
            self.agent = creator;
            self
        }

        pub fn with_builder(mut self, builder: Arc<dyn Build>) -> Self {
            self.builder = builder;
            self
        }

        pub fn with_syncer(mut self, syncer: Arc<dyn Synchronize>) -> Self {
            self.syncer = syncer;
            self
        }

        pub fn with_ids(mut self, ids: Arc<dyn IdStore>) -> Self {
            self.ids = ids;
            self
        }

        pub fn with_debug(mut self, debug: bool) -> Self {
            self.debug = debug;
            self
        }

        pub fn build(self) -> Context {
            let fake_dir =
                icp::prelude::PathBuf::try_from(std::path::PathBuf::from("/fake/test-dir"))
                    .unwrap();
            let fake_artifacts =
                icp::prelude::PathBuf::try_from(std::path::PathBuf::from("/fake/artifacts"))
                    .unwrap();

            Context {
                term: Term::stdout(),
                dirs: Directories::Overridden(fake_dir),
                ids: self.ids,
                artifacts: ArtifactStore::new(&fake_artifacts),
                project: self.project,
                identity: self.identity,
                network: self.network,
                agent: self.agent,
                builder: self.builder,
                syncer: self.syncer,
                debug: self.debug,
            }
        }
    }
}
