use clap::Args;
use icp::identity;

use crate::{
    commands::{Context, Mode},
    options::IdentityOpt,
};

#[derive(Debug, Args)]
pub(crate) struct PrincipalArgs {
    #[command(flatten)]
    pub(crate) identity: IdentityOpt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PrincipalError {
    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("failed to load identity principal: {message}")]
    Sender { message: String },
}

pub(crate) async fn exec(ctx: &Context, args: &PrincipalArgs) -> Result<(), PrincipalError> {
    let id = ctx.identity.load(args.identity.clone().into()).await?;

    let principal = id
        .sender()
        .map_err(|message| PrincipalError::Sender { message })?;

    println!("{principal}");

    Ok(())
}
