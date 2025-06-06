use crate::env::Env;
use clap::{Parser, Subcommand};
use snafu::Snafu;

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
    Default(default::DefaultCmd),
    Import(import::ImportCmd),
    List(list::ListCmd),
    New(new::NewCmd),
    Principal(principal::PrincipalCmd),
}

pub async fn dispatch(env: &Env, cmd: IdentityCmd) -> Result<(), IdentityCommandError> {
    match cmd.subcmd {
        IdentitySubcmd::Default(subcmd) => default::exec(env, subcmd)?,
        IdentitySubcmd::Import(subcmd) => import::exec(env, subcmd)?,
        IdentitySubcmd::List(subcmd) => list::exec(env, subcmd)?,
        IdentitySubcmd::New(subcmd) => new::exec(env, subcmd)?,
        IdentitySubcmd::Principal(subcmd) => principal::exec(env, subcmd)?,
    }
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum IdentityCommandError {
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
