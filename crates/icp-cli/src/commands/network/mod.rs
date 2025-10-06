use clap::{Parser, Subcommand};
use snafu::Snafu;

use crate::commands::Context;

mod list;
mod ping;
mod run;

#[derive(Parser, Debug)]
pub struct NetworkCmd {
    #[command(subcommand)]
    subcmd: NetworkSubcmd,
}

#[derive(Subcommand, Debug)]
pub enum NetworkSubcmd {
    List(list::Cmd),
    Ping(ping::Cmd),
    Run(run::Cmd),
}

pub async fn dispatch(ctx: &Context, cmd: NetworkCmd) -> Result<(), NetworkCommandError> {
    match cmd.subcmd {
        NetworkSubcmd::List(cmd) => list::exec(ctx, cmd).await?,
        NetworkSubcmd::Ping(cmd) => ping::exec(ctx, cmd).await?,
        NetworkSubcmd::Run(cmd) => run::exec(ctx, cmd).await?,
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum NetworkCommandError {
    #[snafu(transparent)]
    List { source: list::CommandError },

    #[snafu(transparent)]
    Ping { source: ping::CommandError },

    #[snafu(transparent)]
    Run { source: run::CommandError },
}
