use clap::{Parser, Subcommand};
use snafu::Snafu;

use crate::context::Context;

mod balance;
mod transfer;

#[derive(Parser, Debug)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Balance(balance::Cmd),
    Transfer(transfer::Cmd),
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    match cmd.subcmd {
        Subcmd::Balance(cmd) => balance::exec(ctx, cmd).await?,
        Subcmd::Transfer(cmd) => transfer::exec(ctx, cmd).await?,
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    Balance { source: balance::CommandError },

    #[snafu(transparent)]
    Transfer { source: transfer::CommandError },
}
