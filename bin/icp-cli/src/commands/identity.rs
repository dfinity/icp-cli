use crate::context::Context;
use clap::{Parser, Subcommand};
use snafu::Snafu;

mod account_id;
mod default;
mod import;
mod list;
mod new;
mod principal;

#[derive(Debug, Parser)]
pub struct IdentityCmd {
    #[command(subcommand)]
    subcmd: IdentitySubcmd,
}

#[derive(Debug, Subcommand)]
pub enum IdentitySubcmd {
    AccountId(account_id::AccountIdCmd),
    Default(default::DefaultCmd),
    Import(import::ImportCmd),
    List(list::ListCmd),
    New(new::NewCmd),
    Principal(principal::PrincipalCmd),
}

pub async fn dispatch(ctx: &Context, cmd: IdentityCmd) -> Result<(), IdentityCommandError> {
    match cmd.subcmd {
        IdentitySubcmd::AccountId(subcmd) => account_id::exec(ctx, subcmd)?,
        IdentitySubcmd::Default(subcmd) => default::exec(ctx, subcmd)?,
        IdentitySubcmd::Import(subcmd) => import::exec(ctx, subcmd)?,
        IdentitySubcmd::List(subcmd) => list::exec(ctx, subcmd)?,
        IdentitySubcmd::New(subcmd) => new::exec(ctx, subcmd)?,
        IdentitySubcmd::Principal(subcmd) => principal::exec(ctx, subcmd)?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum IdentityCommandError {
    #[snafu(transparent)]
    AccountId { source: account_id::AccountIdError },

    #[snafu(transparent)]
    Default {
        source: default::DefaultIdentityError,
    },

    #[snafu(transparent)]
    List { source: list::ListKeysError },

    #[snafu(transparent)]
    Import { source: import::ImportCmdError },

    #[snafu(transparent)]
    New { source: new::NewIdentityError },

    #[snafu(transparent)]
    Principal { source: principal::PrincipalError },
}
