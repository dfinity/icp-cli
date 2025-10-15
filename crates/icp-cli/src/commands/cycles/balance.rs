use snafu::Snafu;

use crate::{commands::Context, commands::token};

pub async fn exec(ctx: &Context, mut cmd: token::balance::Cmd) -> Result<(), CommandError> {
    cmd.token = "cycles".to_string();
    token::balance::exec(ctx, cmd).await.map_err(Into::into)
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    Balance {
        source: token::balance::CommandError,
    },
}
