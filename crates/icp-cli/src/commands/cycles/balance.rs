use crate::{commands::Context, commands::token};

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Balance(#[from] token::balance::CommandError),
}

pub(crate) async fn exec(
    ctx: &Context,
    args: &token::balance::BalanceArgs,
) -> Result<(), CommandError> {
    token::balance::exec(ctx, "cycles", args)
        .await
        .map_err(Into::into)
}
