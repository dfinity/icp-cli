use crate::commands::network::NetworkCommandError;
use clap::{Parser, Subcommand};
use snafu::Snafu;

mod network;

#[derive(Parser, Debug)]
pub struct Cli {
    #[command(subcommand)]
    subcommand: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Network(network::Cmd),
}

pub async fn dispatch(cli: Cli) -> Result<(), DispatchError> {
    match cli.subcommand {
        Subcmd::Network(opts) => network::dispatch(opts).await?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum DispatchError {
    #[snafu(transparent)]
    Network { source: NetworkCommandError },
}
