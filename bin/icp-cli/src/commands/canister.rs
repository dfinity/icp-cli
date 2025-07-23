use clap::{Parser, Subcommand};
use snafu::Snafu;

use crate::context::Context;

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

pub async fn dispatch(ctx: &Context, cmd: Cmd) -> Result<(), CanisterCommandError> {
    match cmd.subcmd {
        CanisterSubcmd::Call(subcmd) => call::exec(ctx, subcmd).await?,
        CanisterSubcmd::Create(subcmd) => create::exec(ctx, subcmd).await?,
        CanisterSubcmd::Delete(subcmd) => delete::exec(ctx, subcmd).await?,
        CanisterSubcmd::Info(subcmd) => info::exec(ctx, subcmd).await?,
        CanisterSubcmd::Install(subcmd) => install::exec(ctx, subcmd).await?,
        CanisterSubcmd::Start(subcmd) => start::exec(ctx, subcmd).await?,
        CanisterSubcmd::Status(subcmd) => status::exec(ctx, subcmd).await?,
        CanisterSubcmd::Stop(subcmd) => stop::exec(ctx, subcmd).await?,
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
