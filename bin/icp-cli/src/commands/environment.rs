use clap::{Parser, Subcommand};
use snafu::Snafu;

mod list;

use crate::context::Context;

#[derive(Parser, Debug)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    List(list::Cmd),
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    match cmd.subcmd {
        Subcmd::List(cmd) => list::exec(ctx, cmd).await?,
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    List { source: list::CommandError },
}
