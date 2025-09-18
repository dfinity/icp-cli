use snafu::Snafu;

use crate::{
    commands::token,
    context::{Context, GetProjectError},
};

pub async fn exec(_ctx: &Context, cmd: token::balance::Cmd) -> Result<(), CommandError> {
    token::balance::exec(_ctx, "cycles", cmd)
        .await
        .map_err(Into::into)
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(transparent)]
    Balance {
        source: token::balance::CommandError,
    },
}
