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
        prelude::*,
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
        let fake_dir = PathBuf::from("/fake/test-dir");
        let fake_artifacts = PathBuf::from("/fake/artifacts");

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

        pub fn with_ids(mut self, ids: Arc<dyn IdStore>) -> Self {
            self.ids = ids;
            self
        }

        pub fn with_network(mut self, network: Arc<dyn Access>) -> Self {
            self.network = network;
            self
        }

        pub fn build(self) -> Context {
            let fake_dir = PathBuf::from("/fake/test-dir");
            let fake_artifacts = PathBuf::from("/fake/artifacts");

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

    #[cfg(test)]
    mod tests {
        use icp::test::create_complex_test_project;

        use super::*;
        use crate::{
            commands::{ContextError, args},
            options::EnvironmentOpt,
        };

        #[test]
        fn test_get_environment_returns_correct_environment() {
            // Setup: Use complex project with multiple environments
            let project = create_complex_test_project();
            let ctx = create_test_context(project);

            // Create ArgContext for "production" environment
            let arg_ctx = args::ArgContext::new_sync(
                &ctx,
                EnvironmentOpt::with_environment("production"),
                None,
                crate::options::IdentityOpt::default(),
                vec![],
            )
            .unwrap();

            // Test: get_environment should return production environment
            let result = ctx.get_environment(&arg_ctx);

            assert!(result.is_ok());
            let env = result.unwrap();
            assert_eq!(env.name, "production");
            assert_eq!(env.network.name, "ic");
        }

        #[test]
        fn test_get_environment_returns_default_local_environment() {
            // Setup: Create project with default "local" environment
            let project = icp::test::create_test_project();
            let ctx = create_test_context(project);

            // Create ArgContext with default (local) environment
            let arg_ctx = args::ArgContext::new_sync(
                &ctx,
                EnvironmentOpt::default(),
                None,
                crate::options::IdentityOpt::default(),
                vec![],
            )
            .unwrap();

            // Test: get_environment should return local environment
            let result = ctx.get_environment(&arg_ctx);

            assert!(result.is_ok());
            let env = result.unwrap();
            assert_eq!(env.name, "local");
            assert_eq!(env.network.name, "local");
        }

        #[test]
        fn test_get_environment_fails_when_environment_not_in_project() {
            // Setup: Create two contexts with different projects
            let project_simple = icp::test::create_test_project(); // Only has "local"
            let project_complex = icp::test::create_complex_test_project(); // Has staging

            let ctx_simple = create_test_context(project_simple);
            let ctx_complex = create_test_context(project_complex);

            // Create ArgContext for "staging" using complex project
            let arg_ctx = args::ArgContext::new_sync(
                &ctx_complex,
                EnvironmentOpt::with_environment("staging"),
                None,
                crate::options::IdentityOpt::default(),
                vec![],
            )
            .unwrap();

            // Test: get_environment should fail because ctx_simple doesn't have staging
            let result = ctx_simple.get_environment(&arg_ctx);

            assert!(result.is_err());
            match result.unwrap_err() {
                ContextError::EnvironmentNotFound { environment } => {
                    assert_eq!(environment, "staging");
                }
                other => panic!("Expected EnvironmentNotFound, got {:?}", other),
            }
        }

        #[test]
        fn test_resolve_canister_id_returns_principal_when_valid_principal_provided() {
            // Setup
            let project = icp::test::create_test_project();
            let ctx = create_test_context(project);

            let arg_ctx = args::ArgContext::new_sync(
                &ctx,
                EnvironmentOpt::default(),
                None,
                crate::options::IdentityOpt::default(),
                vec![],
            )
            .unwrap();

            // Test: resolve_canister_id should return the principal directly
            let principal_text = "aaaaa-aa";
            let result = ctx.resolve_canister_id(&arg_ctx, principal_text);

            assert!(result.is_ok());
            let canister_id = result.unwrap();
            assert_eq!(
                canister_id,
                candid::Principal::from_text(principal_text).unwrap()
            );
        }

        #[test]
        fn test_resolve_canister_id_looks_up_name_in_id_store() {
            // Setup: Create project and context with MockIdStore containing a canister ID
            let project = icp::test::create_test_project();

            let canister_name = "my_backend";
            let canister_id = candid::Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();

            let key = crate::store_id::Key {
                network: "local".to_string(),
                environment: "local".to_string(),
                canister: canister_name.to_string(),
            };

            let id_store = crate::store_id::test::MockIdStore::new().with_id(key, canister_id);

            // Build context with custom ID store
            let ctx = TestContextBuilder::new()
                .with_project(Arc::new(icp::test::MockProjectLoader::new(project)))
                .with_ids(Arc::new(id_store))
                .build();

            let arg_ctx = args::ArgContext::new_sync(
                &ctx,
                EnvironmentOpt::default(),
                None,
                crate::options::IdentityOpt::default(),
                vec![],
            )
            .unwrap();

            // Test: resolve_canister_id should look up the name and return the ID
            let result = ctx.resolve_canister_id(&arg_ctx, canister_name);

            assert!(result.is_ok());
            assert_eq!(result.unwrap(), canister_id);
        }

        #[test]
        fn test_resolve_canister_id_fails_when_name_not_found() {
            // Setup: Context with empty ID store
            let project = icp::test::create_test_project();
            let ctx = create_test_context(project);

            let arg_ctx = args::ArgContext::new_sync(
                &ctx,
                EnvironmentOpt::default(),
                None,
                crate::options::IdentityOpt::default(),
                vec![],
            )
            .unwrap();

            // Test: resolve_canister_id should fail when canister name not found
            let result = ctx.resolve_canister_id(&arg_ctx, "unknown_canister");

            assert!(result.is_err());
            assert!(matches!(
                result.unwrap_err(),
                ContextError::LookupCanisterId(_)
            ));
        }

        #[test]
        fn test_ensure_canister_is_defined_succeeds_when_canister_exists() {
            // Setup: Use complex project which has "backend" canister in all environments
            let project = icp::test::create_complex_test_project();
            let ctx = create_test_context(project);

            let arg_ctx = args::ArgContext::new_sync(
                &ctx,
                EnvironmentOpt::default(),
                None,
                crate::options::IdentityOpt::default(),
                vec![],
            )
            .unwrap();

            // Test: ensure_canister_is_defined should succeed for "backend"
            let result = ctx.ensure_canister_is_defined(&arg_ctx, "backend");

            assert!(result.is_ok());
        }

        #[test]
        fn test_ensure_canister_is_defined_fails_when_canister_not_in_project() {
            // Setup: Project without the canister
            let project = icp::test::create_test_project();
            let ctx = create_test_context(project);

            let arg_ctx = args::ArgContext::new_sync(
                &ctx,
                EnvironmentOpt::default(),
                None,
                crate::options::IdentityOpt::default(),
                vec![],
            )
            .unwrap();

            // Test: ensure_canister_is_defined should fail
            let result = ctx.ensure_canister_is_defined(&arg_ctx, "nonexistent_canister");

            assert!(result.is_err());
            match result.unwrap_err() {
                ContextError::NetworkCanisterNotFound { network, canister } => {
                    assert_eq!(network, "local");
                    assert_eq!(canister, "nonexistent_canister");
                }
                other => panic!("Expected NetworkCanisterNotFound, got {:?}", other),
            }
        }

        #[test]
        fn test_ensure_canister_is_defined_fails_when_canister_not_in_environment() {
            // Setup: Use complex project with "admin" canister
            // "admin" is in project and in "local" env, but NOT in "production" env
            let project = icp::test::create_complex_test_project();
            let ctx = create_test_context(project);

            let arg_ctx = args::ArgContext::new_sync(
                &ctx,
                EnvironmentOpt::with_environment("production"),
                None,
                crate::options::IdentityOpt::default(),
                vec![],
            )
            .unwrap();

            // Test: Should fail because "admin" is not in production environment
            let result = ctx.ensure_canister_is_defined(&arg_ctx, "admin");

            assert!(result.is_err());
            match result.unwrap_err() {
                ContextError::EnvironmentCanisterNotFound {
                    environment,
                    canister,
                } => {
                    assert_eq!(environment, "production");
                    assert_eq!(canister, "admin");
                }
                other => panic!("Expected EnvironmentCanisterNotFound, got {:?}", other),
            }
        }
    }
}
