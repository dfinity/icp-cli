use clap::Args;
use icp::identity;

use icp::context::{Context, GetIdentityError};

use crate::options::IdentityOpt;

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

    #[error(transparent)]
    GetIdentity(#[from] GetIdentityError),
}

pub(crate) async fn exec(ctx: &Context, args: &PrincipalArgs) -> Result<(), PrincipalError> {
    let id = ctx.get_identity(&args.identity.clone().into()).await?;

    let principal = id
        .sender()
        .map_err(|message| PrincipalError::Sender { message })?;

    println!("{principal}");

    Ok(())
}
