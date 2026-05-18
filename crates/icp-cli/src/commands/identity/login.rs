use clap::Args;
use dialoguer::Password;
use icp::{
    context::Context,
    identity::{
        key,
        manifest::{IdentityList, IdentitySpec},
    },
};
use snafu::{OptionExt, ResultExt, Snafu};
use tracing::info;

use crate::commands::identity::link::ii;

/// Re-authenticate an Internet Identity delegation
#[derive(Debug, Args)]
pub(crate) struct LoginArgs {
    /// Name of the identity to re-authenticate
    name: String,
}

pub(crate) async fn exec(ctx: &Context, args: &LoginArgs) -> Result<(), LoginError> {
    let (algorithm, storage, host) = ctx
        .dirs
        .identity()?
        .with_read(async |dirs| {
            let list = IdentityList::load_from(dirs)?;
            let spec = list
                .identities
                .get(&args.name)
                .context(IdentityNotFoundSnafu { name: &args.name })?;
            match spec {
                IdentitySpec::InternetIdentity {
                    algorithm,
                    storage,
                    host,
                    ..
                } => Ok((algorithm.clone(), *storage, host.clone())),
                _ => NotIiSnafu { name: &args.name }.fail(),
            }
        })
        .await??;

    let der_public_key = ctx
        .dirs
        .identity()?
        .with_read(async |dirs| {
            key::load_ii_session_public_key(dirs, &args.name, &algorithm, &storage, || {
                Password::new()
                    .with_prompt("Enter identity password")
                    .interact()
                    .map_err(|e| e.to_string())
            })
        })
        .await?
        .context(LoadSessionKeySnafu)?;

    let chain = ii::recv_delegation(&host, &der_public_key)
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

    #[snafu(display("failed to load II session key"))]
    LoadSessionKey { source: key::LoadIdentityError },

    #[snafu(display("failed during II authentication"))]
    Poll { source: ii::IiRecvError },

    #[snafu(display("failed to update delegation"))]
    UpdateDelegation {
        source: key::UpdateIiDelegationError,
    },
}
