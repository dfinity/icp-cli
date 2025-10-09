use clap::{Parser, Subcommand};

use crate::commands::Context;

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
    Update(Box<update::Cmd>),
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Show(#[from] show::CommandError),

    #[error(transparent)]
    Update(#[from] update::CommandError),
}

pub async fn dispatch(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    match cmd.subcmd {
        Subcmd::Show(cmd) => show::exec(ctx, cmd).await?,
        Subcmd::Update(cmd) => update::exec(ctx, *cmd).await?,
    }

    Ok(())
}
