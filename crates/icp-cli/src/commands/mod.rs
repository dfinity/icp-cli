use std::sync::Arc;

use clap::Subcommand;
use console::Term;
use icp::{
    Directories,
    canister::{build::Build, sync::Synchronize},
    prelude::*,
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

#[derive(Debug, PartialEq)]
pub enum Mode {
    Global,
    Project(PathBuf),
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
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

pub struct Context {
    /// Command exection mode
    pub mode: Mode,

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

    /// Whether debug is enabled
    pub debug: bool,
}
