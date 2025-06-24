use clap::{Parser, Subcommand};
use snafu::Snafu;

use crate::env::Env;

pub mod call;
pub mod create;
pub mod install;
pub mod status;

#[derive(Debug, Parser)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: CanisterSubcmd,
}

#[derive(Debug, Subcommand)]
pub enum CanisterSubcmd {
    Call(call::CanisterCallCmd),
    Create(create::CanisterCreateCmd),
    Install(install::CanisterInstallCmd),
    Status(status::CanisterStatusCmd),
}

pub async fn dispatch(env: &Env, cmd: Cmd) -> Result<(), CanisterCommandError> {
    match cmd.subcmd {
        CanisterSubcmd::Call(subcmd) => call::exec(env, subcmd).await?,
        CanisterSubcmd::Create(subcmd) => create::exec(env, subcmd).await?,
        CanisterSubcmd::Install(subcmd) => install::exec(env, subcmd).await?,
        CanisterSubcmd::Status(subcmd) => status::exec(env, subcmd).await?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterCommandError {
    #[snafu(transparent)]
    Call { source: call::CanisterCallError },

    #[snafu(transparent)]
    Create { source: create::CanisterCreateError },

    #[snafu(transparent)]
    Install {
        source: install::CanisterInstallError,
    },

    #[snafu(transparent)]
    Status { source: status::CanisterStatusError },
}
