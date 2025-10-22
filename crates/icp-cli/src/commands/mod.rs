use std::sync::Arc;

use clap::Subcommand;
use console::Term;
use icp::{
    Directories,
    canister::{build::Build, sync::Synchronize},
};

use candid::Principal;
use ic_agent::Agent;

use crate::{
    commands::args::{ArgValidationError, Environment, Network},
    options::IdentityOpt,
    store_id::Key,
};
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
    pub(crate) async fn get_agent_for_env(
        &self,
        identity: &IdentityOpt,
        environment: &Environment,
    ) -> Result<Agent, ArgValidationError> {
        // Get the environment name
        let ename = match environment {
            Environment::Name(name) => name.clone(),
            Environment::Default(name) => name.clone(),
        };

        // Load project
        let p = self.project.load().await?;

        // Load target environment
        let env = p
            .environments
            .get(&ename)
            .ok_or(ArgValidationError::EnvironmentNotFound {
                name: ename.to_owned(),
            })?;

        // Load identity
        let id = self.identity.load(identity.clone().into()).await?;

        // Access network
        let access = self.network.access(&env.network).await?;

        // Agent
        let agent = self.agent.create(id, &access.url).await?;

        if let Some(k) = access.root_key {
            agent.set_root_key(k);
        }

        Ok(agent)
    }

    pub(crate) async fn get_agent_for_network(
        &self,
        identity: &IdentityOpt,
        network: &Network,
    ) -> Result<Agent, ArgValidationError> {
        match network {
            Network::Name(nname) => {
                let p = self.project.load().await?;

                let network = p
                    .networks
                    .get(nname)
                    .ok_or(ArgValidationError::NetworkNotFound {
                        name: nname.to_string(),
                    })?;

                // Load identity
                let id = self.identity.load(identity.clone().into()).await?;

                // Access network
                let access = self.network.access(network).await?;

                // Agent
                let agent = self.agent.create(id, &access.url).await?;

                if let Some(k) = access.root_key {
                    agent.set_root_key(k);
                }

                Ok(agent)
            }
            Network::Url(url) => {
                let id = self.identity.load(identity.clone().into()).await?;

                // Agent
                let agent = self.agent.create(id, url).await?;

                Ok(agent)
            }
        }
    }

    pub(crate) async fn get_canister_id_for_env(
        &self,
        cname: &String,
        environment: &Environment,
    ) -> Result<Principal, ArgValidationError> {
        // Get the environment name
        let ename = match environment {
            Environment::Name(name) => name.clone(),
            Environment::Default(name) => name.clone(),
        };

        // Load project
        let p = self.project.load().await?;

        // Load target environment
        let env = p
            .environments
            .get(&ename)
            .ok_or(ArgValidationError::EnvironmentNotFound {
                name: ename.to_owned(),
            })?;

        if !env.canisters.contains_key(cname) {
            return Err(ArgValidationError::CanisterNotInEnvironment {
                environment: env.name.to_owned(),
                canister: cname.to_owned(),
            });
        }

        // Lookup the canister id
        let cid = self.ids.lookup(&Key {
            network: env.network.name.to_owned(),
            environment: env.name.to_owned(),
            canister: cname.to_owned(),
        })?;

        Ok(cid)
    }
}
