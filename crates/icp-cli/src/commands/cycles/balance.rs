use snafu::Snafu;

use crate::{commands::Context, commands::token};

pub async fn exec(ctx: &Context, cmd: token::balance::Cmd) -> Result<(), CommandError> {
    token::balance::exec(ctx, "cycles", cmd)
        .await
        .map_err(Into::into)
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    Balance {
        source: token::balance::CommandError,
    },
}
