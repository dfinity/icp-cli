use clap::Args;
use icp::identity;

use crate::{
    commands::{Context, Mode},
    options::IdentityOpt,
};

#[derive(Debug, Args)]
pub struct PrincipalArgs {
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

pub async fn exec(ctx: &Context, args: &PrincipalArgs) -> Result<(), PrincipalError> {
    match &ctx.mode {
        Mode::Global | Mode::Project(_) => {
            let id = ctx.identity.load(args.identity.clone().into()).await?;

            let principal = id
                .sender()
                .map_err(|message| PrincipalError::Sender { message })?;

            println!("{principal}");
        }
    }

    Ok(())
}
