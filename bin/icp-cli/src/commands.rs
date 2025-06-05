use crate::commands::{build::BuildCommandError, network::NetworkCommandError};
use clap::{Parser, Subcommand};
use snafu::Snafu;

mod build;
mod network;

#[derive(Parser, Debug)]
pub struct Cli {
    #[command(subcommand)]
    subcommand: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Build(build::Cmd),
    Network(network::Cmd),
}

pub async fn dispatch(cli: Cli) -> Result<(), DispatchError> {
    match cli.subcommand {
        Subcmd::Build(opts) => build::exec(opts).await?,
        Subcmd::Network(opts) => network::dispatch(opts).await?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum DispatchError {
    #[snafu(transparent)]
    Build { source: BuildCommandError },

    #[snafu(transparent)]
    Network { source: NetworkCommandError },
}
