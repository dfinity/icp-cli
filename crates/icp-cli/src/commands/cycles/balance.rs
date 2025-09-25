use snafu::Snafu;

use crate::{
    commands::token,
    context::{Context, ContextProjectError},
};

pub async fn exec(ctx: &Context, cmd: token::balance::Cmd) -> Result<(), CommandError> {
    token::balance::exec(ctx, "cycles", cmd)
        .await
        .map_err(Into::into)
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: ContextProjectError },

    #[snafu(transparent)]
    Balance {
        source: token::balance::CommandError,
    },
}
