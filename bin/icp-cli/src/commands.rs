use crate::{
    commands::{canister::CanisterCommandError, network::NetworkCommandError},
    env::Env,
};
use clap::{Parser, Subcommand};
use identity::IdentityCommandError;
use snafu::Snafu;

mod build;
mod canister;
mod deploy;
mod identity;
mod network;
mod sync;

#[derive(Parser, Debug)]
pub struct Cmd {
    #[command(subcommand)]
    subcommand: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Build(build::Cmd),
    Canister(canister::Cmd),
    Deploy(deploy::Cmd),
    Identity(identity::IdentityCmd),
    Network(network::NetworkCmd),
    Sync(sync::Cmd),
}

pub async fn dispatch(env: &Env, cli: Cmd) -> Result<(), DispatchError> {
    match cli.subcommand {
        Subcmd::Build(opts) => build::exec(env, opts).await?,
        Subcmd::Canister(opts) => canister::dispatch(env, opts).await?,
        Subcmd::Deploy(opts) => deploy::exec(env, opts).await?,
        Subcmd::Identity(opts) => identity::dispatch(env, opts).await?,
        Subcmd::Network(opts) => network::dispatch(env, opts).await?,
        Subcmd::Sync(opts) => sync::exec(env, opts).await?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum DispatchError {
    #[snafu(transparent)]
    Build { source: build::CommandError },

    #[snafu(transparent)]
    Canister { source: CanisterCommandError },

    #[snafu(transparent)]
    Deploy { source: deploy::CommandError },

    #[snafu(transparent)]
    Identity { source: IdentityCommandError },

    #[snafu(transparent)]
    Network { source: NetworkCommandError },

    #[snafu(transparent)]
    Sync { source: sync::CommandError },
}
