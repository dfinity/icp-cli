use clap::{Parser, Subcommand};
use snafu::Snafu;

use crate::env::Env;

pub mod call;
pub mod create;
pub mod delete;
pub mod info;
pub mod install;
pub mod start;
pub mod status;
pub mod stop;

#[derive(Debug, Parser)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: CanisterSubcmd,
}

#[derive(Debug, Subcommand)]
pub enum CanisterSubcmd {
    Call(call::CanisterCallCmd),
    Create(create::CanisterCreateCmd),
    Delete(delete::CanisterDeleteCmd),
    Info(info::CanisterInfoCmd),
    Install(install::CanisterInstallCmd),
    Start(start::CanisterStartCmd),
    Status(status::CanisterStatusCmd),
    Stop(stop::CanisterStopCmd),
}

pub async fn dispatch(env: &Env, cmd: Cmd) -> Result<(), CanisterCommandError> {
    match cmd.subcmd {
        CanisterSubcmd::Call(subcmd) => call::exec(env, subcmd).await?,
        CanisterSubcmd::Create(subcmd) => create::exec(env, subcmd).await?,
        CanisterSubcmd::Delete(subcmd) => delete::exec(env, subcmd).await?,
        CanisterSubcmd::Info(subcmd) => info::exec(env, subcmd).await?,
        CanisterSubcmd::Install(subcmd) => install::exec(env, subcmd).await?,
        CanisterSubcmd::Start(subcmd) => start::exec(env, subcmd).await?,
        CanisterSubcmd::Status(subcmd) => status::exec(env, subcmd).await?,
        CanisterSubcmd::Stop(subcmd) => stop::exec(env, subcmd).await?,
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
    Delete { source: delete::CanisterDeleteError },

    #[snafu(transparent)]
    Start { source: start::CanisterStartError },

    #[snafu(transparent)]
    Info { source: info::CanisterInfoError },

    #[snafu(transparent)]
    Install {
        source: install::CanisterInstallError,
    },

    #[snafu(transparent)]
    Status { source: status::CanisterStatusError },

    #[snafu(transparent)]
    Stop { source: stop::CanisterStopError },
}
