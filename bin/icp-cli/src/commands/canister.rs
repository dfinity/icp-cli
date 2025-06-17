use clap::{Parser, Subcommand};
use snafu::Snafu;

use crate::env::Env;

mod create;
mod install;

#[derive(Debug, Parser)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: CanisterSubcmd,
}

#[derive(Debug, Subcommand)]
pub enum CanisterSubcmd {
    Create(create::CanisterCreateCmd),
    Install(install::CanisterInstallCmd),
}

pub async fn dispatch(env: &Env, cmd: Cmd) -> Result<(), CanisterCommandError> {
    match cmd.subcmd {
        CanisterSubcmd::Create(subcmd) => create::exec(env, subcmd)?,
        CanisterSubcmd::Install(subcmd) => install::exec(env, subcmd)?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterCommandError {
    #[snafu(transparent)]
    Create { source: create::CanisterCreateError },

    #[snafu(transparent)]
    Install {
        source: install::CanisterInstallError,
    },
}
