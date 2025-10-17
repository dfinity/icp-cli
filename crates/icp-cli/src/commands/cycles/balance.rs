use crate::{commands::Context, commands::token};

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Balance(#[from] token::balance::CommandError),
}

pub async fn exec(ctx: &Context, args: &token::balance::BalanceArgs) -> Result<(), CommandError> {
    token::balance::exec(ctx, "cycles", args)
        .await
        .map_err(Into::into)
}
