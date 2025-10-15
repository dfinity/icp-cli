use std::sync::Arc;

use clap::Subcommand;
use console::Term;
use icp::{
    Directories,
    canister::{build::Build, sync::Synchronize},
    manifest::Locate,
};

use crate::{store_artifact::ArtifactStore, store_id::IdStore};

pub mod build;
pub mod canister;
pub mod cycles;
pub mod deploy;
pub mod environment;
pub mod identity;
pub mod network;
pub mod project;
pub mod sync;
pub mod token;

pub struct Context {
    /// Workspace locator
    pub workspace: Arc<dyn Locate>,

    /// Terminal for printing messages for the user to see
    pub term: Term,

    /// Various cli-related directories (cache, configuration, etc).
    pub dirs: Directories,

    /// Canisters ID Store for lookup and storage
    pub ids: IdStore,

    /// An artifact store for canister build artifacts
    pub artifacts: ArtifactStore,

    /// Project loader
    pub project: Arc<dyn icp::Load>,

    /// Identity loader
    pub identity: Arc<dyn icp::identity::Load>,

    /// NetworkAccess loader
    pub network: Arc<dyn icp::network::Access>,

    /// Agent creator
    pub agent: Arc<dyn icp::agent::Create>,

    /// Canister builder
    pub builder: Arc<dyn Build>,

    /// Canister synchronizer
    pub syncer: Arc<dyn Synchronize>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Build a project
    Build(build::Cmd),

    /// Perform canister operations against a network
    Canister(canister::Cmd),

    /// Mint and manage cycles
    #[command(subcommand)]
    Cycles(cycles::Command),

    /// Deploy a project to an environment
    Deploy(deploy::Cmd),

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
    Sync(sync::Cmd),

    /// Perform token transactions
    #[command(subcommand)]
    Token(token::Command),
}
