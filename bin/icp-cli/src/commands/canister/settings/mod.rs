use clap::{Parser, Subcommand};
use snafu::Snafu;

use crate::context::Context;

pub mod show;
pub mod update;

#[derive(Parser, Debug)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: Subcmd,
}

#[derive(Subcommand, Debug)]
pub enum Subcmd {
    Show(show::Cmd),
    Update(update::Cmd),
}

pub async fn dispatch(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    match cmd.subcmd {
        Subcmd::Show(cmd) => show::exec(ctx, cmd).await?,
        Subcmd::Update(cmd) => update::exec(ctx, cmd).await?,
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    Show { source: show::CommandError },

    #[snafu(transparent)]
    Update { source: update::CommandError },
}
