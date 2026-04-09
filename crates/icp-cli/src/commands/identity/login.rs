use clap::Args;
use icp::{
    context::Context,
    identity::{
        key,
        manifest::{IdentityList, IdentitySpec},
    },
};
use snafu::{OptionExt, ResultExt, Snafu};
use tracing::info;

use crate::operations::ii_poll;

/// Re-authenticate an Internet Identity delegation
#[derive(Debug, Args)]
pub(crate) struct LoginArgs {
    /// Name of the identity to re-authenticate
    name: String,
}

pub(crate) async fn exec(ctx: &Context, args: &LoginArgs) -> Result<(), LoginError> {
    let algorithm = ctx
        .dirs
        .identity()?
        .with_read(async |dirs| {
            let list = IdentityList::load_from(dirs)?;
            let spec = list
                .identities
                .get(&args.name)
                .context(IdentityNotFoundSnafu { name: &args.name })?;
            match spec {
                IdentitySpec::InternetIdentity { algorithm, .. } => Ok(algorithm.clone()),
                _ => NotIiSnafu { name: &args.name }.fail(),
            }
        })
        .await??;

    let der_public_key =
        key::load_ii_session_public_key(&args.name, &algorithm).context(LoadSessionKeySnafu)?;

    let chain = ii_poll::poll_for_delegation(&der_public_key)
        .await
        .context(PollSnafu)?;

    ctx.dirs
        .identity()?
        .with_write(async |dirs| key::update_ii_delegation(dirs, &args.name, &chain))
        .await?
        .context(UpdateDelegationSnafu)?;

    info!("Identity `{}` re-authenticated", args.name);

    Ok(())
}

#[derive(Debug, Snafu)]
pub(crate) enum LoginError {
    #[snafu(transparent)]
    LockIdentityDir { source: icp::fs::lock::LockError },

    #[snafu(transparent)]
    LoadManifest {
        source: icp::identity::manifest::LoadIdentityManifestError,
    },

    #[snafu(display("no identity found with name `{name}`"))]
    IdentityNotFound { name: String },

    #[snafu(display(
        "identity `{name}` is not an Internet Identity; use `icp identity link ii` instead"
    ))]
    NotIi { name: String },

    #[snafu(display("failed to load II session key from keyring"))]
    LoadSessionKey { source: key::LoadIdentityError },

    #[snafu(display("failed during II authentication"))]
    Poll { source: ii_poll::IiPollError },

    #[snafu(display("failed to update delegation"))]
    UpdateDelegation {
        source: key::UpdateIiDelegationError,
    },
}
