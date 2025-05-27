use crate::commands::network::run::RunNetworkCommandError;
use clap::{Parser, Subcommand};
use snafu::Snafu;

mod run;

#[derive(Parser, Debug)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Run(run::Cmd),
}

pub async fn dispatch(cmd: Cmd) -> Result<(), NetworkCommandError> {
    match cmd.subcmd {
        Subcmd::Run(cmd) => run::exec(cmd).await?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum NetworkCommandError {
    #[snafu(transparent)]
    Run { source: RunNetworkCommandError },
}
