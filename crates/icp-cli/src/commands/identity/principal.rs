use clap::Parser;
use icp::identity;
use tracing::info;

use crate::{commands::Context, options::IdentityOpt};

#[derive(Debug, Parser)]
pub struct PrincipalCmd {
    #[command(flatten)]
    pub identity: IdentityOpt,
}

#[derive(Debug, thiserror::Error)]
pub enum PrincipalError {
    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("failed to load identity principal: {message}")]
    Sender { message: String },
}

pub async fn exec(ctx: &Context, cmd: PrincipalCmd) -> Result<(), PrincipalError> {
    let id = ctx.identity.load(cmd.identity.into()).await?;

    let principal = id
        .sender()
        .map_err(|message| PrincipalError::Sender { message })?;

    info!("{principal}");

    Ok(())
}
