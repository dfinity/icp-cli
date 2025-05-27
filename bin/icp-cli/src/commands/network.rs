use crate::commands::network::run::RunNetworkError;
use clap::{Parser, Subcommand};
use snafu::Snafu;

mod run;
mod start;

#[derive(Parser, Debug)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Run(run::Cmd),
}

#[derive(Debug, Snafu)]
pub enum NetworkCommandError {
    #[snafu(transparent)]
    Run { source: RunNetworkError },
}
pub async fn dispatch(cmd: Cmd) -> Result<(), NetworkCommandError> {
    match cmd.subcmd {
        Subcmd::Run(cmd) => run::exec(cmd).await?,
    }
    Ok(())
}
