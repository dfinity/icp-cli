use crate::{commands::network::run::RunNetworkCommandError, env::Env};
use clap::{Parser, Subcommand};
use snafu::Snafu;

mod run;

#[derive(Parser, Debug)]
pub struct NetworkCmd {
    #[command(subcommand)]
    subcmd: NetworkSubcmd,
}

#[derive(Subcommand, Debug)]
pub enum NetworkSubcmd {
    Run(run::Cmd),
}

pub async fn dispatch(_env: &Env, cmd: NetworkCmd) -> Result<(), NetworkCommandError> {
    match cmd.subcmd {
        NetworkSubcmd::Run(cmd) => run::exec(cmd).await?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum NetworkCommandError {
    #[snafu(transparent)]
    Run { source: RunNetworkCommandError },
}
