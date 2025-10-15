use crate::{commands::Context, commands::token};

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Balance(#[from] token::balance::CommandError),
}

pub async fn exec(ctx: &Context, args: &token::balance::BalanceArgs) -> Result<(), CommandError> {
    let mut args = args.to_owned();
    args.token = "cycles".to_string();
    token::balance::exec(ctx, &args).await.map_err(Into::into)
}
