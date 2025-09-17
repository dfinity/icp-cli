use clap::Parser;
use snafu::Snafu;

use crate::{
    commands::token,
    context::Context,
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Debug, Parser)]
pub struct Cmd {
    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    Ok(token::balance::exec(
        ctx,
        "cycles",
        token::balance::Cmd {
            identity: cmd.identity,
            environment: cmd.environment,
        },
    )
    .await?)
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    TokenBalance {
        source: token::balance::CommandError,
    },
}
