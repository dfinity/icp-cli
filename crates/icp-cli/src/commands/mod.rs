use std::{cell::RefCell, sync::Arc};

use candid::Principal;
use clap::Subcommand;
use console::Term;
use ic_agent::Agent;
use icp::{
    Directories, Environment,
    canister::{build::Build, sync::Synchronize},
    prelude::*,
};

use crate::{
    options::{EnvironmentOpt, IdentityOpt},
    store_artifact::ArtifactStore,
    store_id::{IdStore, Key},
};

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

#[derive(Debug, PartialEq)]
pub(crate) enum Mode {
    Global,
    Project(PathBuf),
}

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

pub(crate) struct UserOptions {
    pub(crate) identity: RefCell<Option<IdentityOpt>>,
    pub(crate) environment: RefCell<Option<EnvironmentOpt>>,
}

impl Default for UserOptions {
    fn default() -> Self {
        Self {
            identity: RefCell::new(None),
            environment: RefCell::new(None),
        }
    }
}

pub(crate) struct Context {
    /// Command exection mode
    pub(crate) mode: Mode,

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

    /// User options
    pub(crate) user: UserOptions,
}

impl Context {
    pub(crate) fn with_identity(&self, identity: IdentityOpt) {
        let mut identity_ref = self.user.identity.borrow_mut();
        if identity_ref.is_some() {
            panic!("Identity already set");
        }
        *identity_ref = Some(identity);
    }

    pub(crate) fn with_environment(&self, environment: EnvironmentOpt) {
        let mut environment_ref = self.user.environment.borrow_mut();
        if environment_ref.is_some() {
            panic!("Environment already set");
        }
        *environment_ref = Some(environment);
    }

    pub(crate) fn get_id_choice(&self) -> Result<IdentityOpt, String> {
        match self.mode {
            Mode::Project(_) => match self.user.identity.borrow().clone() {
                Some(identity_choice) => Ok(identity_choice),
                None => Err("Bug: Identity not set".to_string()),
            },
            Mode::Global => match self.user.identity.borrow().clone() {
                Some(_) => Err("Identity cannot be set outside of a project".to_string()),
                None => Ok(IdentityOpt::default()),
            },
        }
    }

    pub(crate) fn get_environment_choice(&self) -> Result<EnvironmentOpt, String> {
        match self.mode {
            Mode::Project(_) => match self.user.environment.borrow().clone() {
                Some(environment_choice) => Ok(environment_choice),
                None => Err("Bug: Environment not set".to_string()),
            },
            Mode::Global => match self.user.environment.borrow().clone() {
                Some(_) => Err("Environment cannot be set outside of a project".to_string()),
                None => Ok(EnvironmentOpt::default()),
            },
        }
    }

    pub(crate) async fn get_environment(&self) -> Result<Environment, String> {
        let environment_choice = self.get_environment_choice()?;
        let project = self.project.load().await.map_err(|e| e.to_string())?;
        let env = project
            .environments
            .get(environment_choice.name())
            .ok_or(format!(
                "Environment {} not found",
                environment_choice.name()
            ))?
            .clone();
        Ok(env)
    }

    pub(crate) async fn get_canister_principal(&self, name: &str) -> Result<Principal, String> {
        if let Ok(canister) = Principal::from_text(name) {
            return Ok(canister);
        }

        let environment = self.get_environment().await?;
        environment.canisters.get(name).ok_or(format!(
            "Canister {} not found in environment {}",
            name, environment.name
        ))?;

        let key = Key {
            network: environment.network.name.clone(),
            environment: environment.name.clone(),
            canister: name.to_string(),
        };

        self.ids.lookup(&key).map_err(|e| e.to_string())
    }

    pub(crate) async fn get_agent(&self) -> Result<Agent, String> {
        let id_choice = self.get_id_choice()?;

        let env = self.get_environment().await?;

        let access = self
            .network
            .access(&env.network)
            .await
            .map_err(|e| e.to_string())?;

        let id = self
            .identity
            .load(id_choice.into())
            .await
            .map_err(|e| e.to_string())?;

        let agent = self
            .agent
            .create(id, &access.url)
            .await
            .map_err(|e| e.to_string())?;

        if let Some(k) = access.root_key {
            agent.set_root_key(k);
        }

        Ok(agent)
    }
}
