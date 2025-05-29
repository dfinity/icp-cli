use crate::{commands::network::NetworkCommandError, env::Env};
use clap::{Parser, Subcommand};
use identity::IdentityCommandError;
use snafu::Snafu;

mod identity;
mod network;

#[derive(Parser, Debug)]
pub struct Cmd {
    #[command(subcommand)]
    subcommand: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Identity(identity::IdentityCmd),
    Network(network::NetworkCmd),
}

pub async fn dispatch(env: &Env, cli: Cmd) -> Result<(), DispatchError> {
    match cli.subcommand {
        Subcmd::Identity(opts) => identity::dispatch(env, opts).await?,
        Subcmd::Network(opts) => network::dispatch(env, opts).await?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum DispatchError {
    #[snafu(transparent)]
    Identity { source: IdentityCommandError },
    #[snafu(transparent)]
    Network { source: NetworkCommandError },
}
