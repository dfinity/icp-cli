use clap::{Parser, Subcommand};
use snafu::Snafu;

use crate::{commands::token, commands::Context};

mod balance;
mod mint;

#[derive(Parser, Debug)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Balance(token::balance::Cmd),
    Mint(mint::Cmd),
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    match cmd.subcmd {
        Subcmd::Balance(cmd) => balance::exec(ctx, cmd).await?,
        Subcmd::Mint(cmd) => mint::exec(ctx, cmd).await?,
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    Balance { source: balance::CommandError },

    #[snafu(transparent)]
    Mint { source: mint::CommandError },
}
