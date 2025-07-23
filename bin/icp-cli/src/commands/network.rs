use crate::{
    commands::network::{ping::PingNetworkCommandError, run::RunNetworkCommandError},
    context::Context,
};
use clap::{Parser, Subcommand};
use snafu::Snafu;

mod ping;
mod run;

#[derive(Parser, Debug)]
pub struct NetworkCmd {
    #[command(subcommand)]
    subcmd: NetworkSubcmd,
}

#[derive(Subcommand, Debug)]
pub enum NetworkSubcmd {
    Ping(ping::PingCmd),
    Run(run::Cmd),
}

pub async fn dispatch(ctx: &Context, cmd: NetworkCmd) -> Result<(), NetworkCommandError> {
    match cmd.subcmd {
        NetworkSubcmd::Ping(cmd) => ping::exec(ctx, cmd).await?,
        NetworkSubcmd::Run(cmd) => run::exec(ctx, cmd).await?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum NetworkCommandError {
    #[snafu(transparent)]
    Ping { source: PingNetworkCommandError },

    #[snafu(transparent)]
    Run { source: RunNetworkCommandError },
}
