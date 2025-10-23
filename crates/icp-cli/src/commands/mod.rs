use std::sync::Arc;

use anyhow::{Context as _, anyhow, bail};
use clap::Subcommand;
use console::Term;
use icp::{
    Directories,
    canister::{build::Build, sync::Synchronize},
    identity::IdentitySelection,
    network::access::NetworkAccess,
};

use candid::Principal;
use ic_agent::{Agent, Identity};

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

type ContextResult<T> = anyhow::Result<T>;

impl Context {
    /// Gets an identity based on the provided identity selection.
    // TODO: refactor the whole codebase to use this method instead of directly accessing `ctx.identity.load()`
    pub(crate) async fn get_identity(
        &self,
        identity: &IdentitySelection,
    ) -> ContextResult<Arc<dyn Identity>> {
        Ok(self.identity.load(identity.clone()).await?)
    }

    /// Gets an environment by name from the currently loaded project.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be loaded or if the environment is not found.
    pub(crate) async fn get_environment(
        &self,
        environment_name: &str,
    ) -> ContextResult<icp::Environment> {
        // Load project
        let p = self
            .project
            .load()
            .await
            .context("failed to load project which is required to get environment")?;

        // Load target environment
        let env = p
            .environments
            .get(environment_name)
            .ok_or(anyhow!("environment not found: {}", environment_name))?;

        Ok(env.clone())
    }

    /// Gets an Network by name from the currently loaded project.
    ///
    /// # Errors
    ///
    /// Returns an error if the project cannot be loaded or if the network is not found.
    pub(crate) async fn get_network(&self, network_name: &str) -> ContextResult<icp::Network> {
        // Load project
        let p = self
            .project
            .load()
            .await
            .context("failed to load project which is required to get network")?;

        // Load target network
        let net = p
            .networks
            .get(network_name)
            .ok_or(anyhow!("network not found: {}", network_name))?;

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
    ) -> ContextResult<Principal> {
        let env = self.get_environment(environment_name).await?;

        if !env.canisters.contains_key(canister_name) {
            bail!(
                "canister '{}' not found in environment '{}'",
                canister_name,
                env.name
            );
        }

        // Lookup the canister id
        let cid = self.ids.lookup(&Key {
            network: env.network.name.to_owned(),
            environment: env.name.to_owned(),
            canister: canister_name.to_owned(),
        })?;

        Ok(cid)
    }

    /// Creates an agent for a given identity and environment.
    pub(crate) async fn get_agent_for_env(
        &self,
        identity: &IdentitySelection,
        environment_name: &str,
    ) -> ContextResult<Agent> {
        let id = self.get_identity(identity).await?;
        let env = self.get_environment(environment_name).await?;
        let access = self.network.access(&env.network).await?;
        self.create_agent(id, access).await
    }

    /// Creates an agent for a given identity and network.
    pub(crate) async fn get_agent_for_network(
        &self,
        identity: &IdentitySelection,
        network_name: &str,
    ) -> ContextResult<Agent> {
        let id = self.get_identity(identity).await?;
        let network = self.get_network(network_name).await?;
        let access = self.network.access(&network).await?;
        self.create_agent(id, access).await
    }

    /// Private helper to create an agent given identity and network access.
    ///
    /// Used by [`Self::get_agent_for_env`] and [`Self::get_agent_for_network`].
    async fn create_agent(
        &self,
        id: Arc<dyn Identity>,
        network_access: NetworkAccess,
    ) -> ContextResult<Agent> {
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
    ) -> ContextResult<Agent> {
        let id = self.get_identity(identity).await?;
        let agent = self.agent.create(id, url).await?;
        Ok(agent)
    }
}
