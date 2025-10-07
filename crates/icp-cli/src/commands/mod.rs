use std::sync::Arc;

use crate::{
    commands::{canister::CanisterCommandError, network::NetworkCommandError},
    store_artifact::ArtifactStore,
    store_id::IdStore,
};
use clap::{Parser, Subcommand};
use console::Term;
use icp::Directories;
use identity::IdentityCommandError;
use snafu::Snafu;

mod build;
mod canister;
mod cycles;
mod deploy;
mod environment;
mod identity;
mod network;
mod sync;
mod token;

pub struct Context {
    /// Terminal for printing messages for the user to see
    pub term: Term,

    /// Various cli-related directories (cache, configuration, etc).
    pub dirs: Directories,

    /// Canisters ID Store for lookup and storage
    pub id_store: IdStore,

    /// An artifact store for canister build artifacts
    pub artifact_store: ArtifactStore,

    /// Project loader
    pub project: Arc<dyn icp::Load>,

    /// Identity loader
    pub identity: Arc<dyn icp::identity::Load>,

    /// NetworkAccess loader
    pub network: Arc<dyn icp::network::Access>,

    /// Agent creator
    pub agent: Arc<dyn icp::agent::Create>,
}

#[derive(Parser, Debug)]
pub struct Cmd {
    #[command(subcommand)]
    subcommand: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Build(build::Cmd),
    Canister(canister::Cmd),
    Cycles(cycles::Cmd),
    Deploy(deploy::Cmd),
    Environment(environment::Cmd),
    Identity(identity::IdentityCmd),
    Network(network::NetworkCmd),
    Sync(sync::Cmd),
    Token(token::Cmd),
}

#[derive(Debug, Snafu)]
pub enum DispatchError {
    #[snafu(transparent)]
    Build { source: build::CommandError },

    #[snafu(transparent)]
    Canister { source: CanisterCommandError },

    #[snafu(transparent)]
    Cycles { source: cycles::CommandError },

    #[snafu(transparent)]
    Deploy { source: deploy::CommandError },

    #[snafu(transparent)]
    Environment { source: environment::CommandError },

    #[snafu(transparent)]
    Identity { source: IdentityCommandError },

    #[snafu(transparent)]
    Network { source: NetworkCommandError },

    #[snafu(transparent)]
    Sync { source: sync::CommandError },

    #[snafu(transparent)]
    Token { source: token::CommandError },
}

pub async fn dispatch(ctx: &Context, subcmd: Subcmd) -> Result<(), DispatchError> {
    match subcmd {
        Subcmd::Build(opts) => build::exec(ctx, opts).await?,
        Subcmd::Canister(opts) => canister::dispatch(ctx, opts).await?,
        Subcmd::Cycles(opts) => cycles::exec(ctx, opts).await?,
        Subcmd::Deploy(opts) => deploy::exec(ctx, opts).await?,
        Subcmd::Environment(opts) => environment::exec(ctx, opts).await?,
        Subcmd::Identity(opts) => identity::dispatch(ctx, opts).await?,
        Subcmd::Network(opts) => network::dispatch(ctx, opts).await?,
        Subcmd::Sync(opts) => sync::exec(ctx, opts).await?,
        Subcmd::Token(opts) => token::exec(ctx, opts).await?,
    }
    Ok(())
}
