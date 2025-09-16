use clap::{Parser, Subcommand};
use snafu::Snafu;

use crate::context::Context;

pub mod call;
pub mod create;
pub mod delete;
pub mod info;
pub mod install;
pub mod list;
pub mod show;
pub mod start;
pub mod status;
pub mod stop;
pub mod update_settings;

#[derive(Debug, Parser)]
pub struct Cmd {
    #[command(subcommand)]
    subcmd: CanisterSubcmd,
}

#[derive(Debug, Subcommand)]
pub enum CanisterSubcmd {
    Call(call::Cmd),
    Create(create::Cmd),
    Delete(delete::Cmd),
    Info(info::Cmd),
    Install(install::Cmd),
    Show(show::Cmd),
    List(list::Cmd),
    Start(start::Cmd),
    Status(status::Cmd),
    Stop(stop::Cmd),
    UpdateSettings(update_settings::Cmd),
}

pub async fn dispatch(ctx: &Context, cmd: Cmd) -> Result<(), CanisterCommandError> {
    match cmd.subcmd {
        CanisterSubcmd::Call(subcmd) => call::exec(ctx, subcmd).await?,
        CanisterSubcmd::Create(subcmd) => create::exec(ctx, subcmd).await?,
        CanisterSubcmd::Delete(subcmd) => delete::exec(ctx, subcmd).await?,
        CanisterSubcmd::Info(subcmd) => info::exec(ctx, subcmd).await?,
        CanisterSubcmd::Install(subcmd) => install::exec(ctx, subcmd).await?,
        CanisterSubcmd::List(subcmd) => list::exec(ctx, subcmd).await?,
        CanisterSubcmd::Start(subcmd) => start::exec(ctx, subcmd).await?,
        CanisterSubcmd::Show(subcmd) => show::exec(ctx, subcmd).await?,
        CanisterSubcmd::Status(subcmd) => status::exec(ctx, subcmd).await?,
        CanisterSubcmd::Stop(subcmd) => stop::exec(ctx, subcmd).await?,
        CanisterSubcmd::UpdateSettings(subcmd) => update_settings::exec(ctx, subcmd).await?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterCommandError {
    #[snafu(transparent)]
    Call { source: call::CommandError },

    #[snafu(transparent)]
    Create { source: create::CommandError },

    #[snafu(transparent)]
    Delete { source: delete::CommandError },

    #[snafu(transparent)]
    Start { source: start::CommandError },

    #[snafu(transparent)]
    Info { source: info::CommandError },

    #[snafu(transparent)]
    Install { source: install::CommandError },

    #[snafu(transparent)]
    Show { source: show::CommandError },

    #[snafu(transparent)]
    List { source: list::CommandError },

    #[snafu(transparent)]
    Status { source: status::CommandError },

    #[snafu(transparent)]
    Stop { source: stop::CommandError },

    #[snafu(transparent)]
    UpdateSettings {
        source: update_settings::CommandError,
    },
}
