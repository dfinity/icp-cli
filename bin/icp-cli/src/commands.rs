use crate::{
    commands::{build::BuildCommandError, network::NetworkCommandError},
    env::Env,
};
use clap::{Parser, Subcommand};
use identity::IdentityCommandError;
use snafu::Snafu;

mod build;
mod identity;
mod network;

#[derive(Parser, Debug)]
pub struct Cmd {
    #[command(subcommand)]
    subcommand: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Build(build::Cmd),
    Identity(identity::IdentityCmd),
    Network(network::NetworkCmd),
}

pub async fn dispatch(env: &Env, cli: Cmd) -> Result<(), DispatchError> {
    match cli.subcommand {
        Subcmd::Build(opts) => build::exec(opts).await?,
        Subcmd::Identity(opts) => identity::dispatch(env, opts).await?,
        Subcmd::Network(opts) => network::dispatch(env, opts).await?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum DispatchError {
    #[snafu(transparent)]
    Build { source: BuildCommandError },

    #[snafu(transparent)]
    Identity { source: IdentityCommandError },

    #[snafu(transparent)]
    Network { source: NetworkCommandError },
}
