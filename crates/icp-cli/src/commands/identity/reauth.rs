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

use crate::commands::identity::link::web;

/// Re-authenticate a delegation-based identity
#[derive(Debug, Args)]
pub(crate) struct ReauthArgs {
    /// Name of the identity to re-authenticate
    name: String,
}

pub(crate) async fn exec(ctx: &Context, args: &ReauthArgs) -> Result<(), LoginError> {
    let (algorithm, storage, host, domain, principal) = ctx
        .dirs
        .identity()?
        .with_read(async |dirs| {
            let list = IdentityList::load_from(dirs)?;
            let spec = list
                .identities
                .get(&args.name)
                .context(IdentityNotFoundSnafu { name: &args.name })?;
            match spec {
                IdentitySpec::WebAuth {
                    algorithm,
                    principal,
                    storage,
                    host,
                    domain,
                } => Ok((
                    algorithm.clone(),
                    *storage,
                    host.clone(),
                    domain.clone(),
                    *principal,
                )),
                _ => NotDelegationSnafu { name: &args.name }.fail(),
            }
        })
        .await??;

    let der_public_key = ctx
        .dirs
        .identity()?
        .with_read(async |dirs| {
            key::load_webauth_session_public_key(dirs, &args.name, &algorithm, &storage, || {
                Password::new()
                    .with_prompt("Enter identity password")
                    .interact()
                    .map_err(|e| e.to_string())
            })
        })
        .await?
        .context(LoadSessionKeySnafu)?;

    // Re-auth must resolve to the same web-auth principal that was originally linked,
    // so reuse the delegation domain captured at link time.
    let chain = web::recv_delegation(&host, domain.as_deref(), &der_public_key, Some(principal))
        .await
        .context(PollSnafu)?;

    ctx.dirs
        .identity()?
        .with_write(async |dirs| key::update_webauth_delegation(dirs, &args.name, &chain))
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
        "identity `{name}` is not delegation-based; this command is not required to use it"
    ))]
    NotDelegation { name: String },

    #[snafu(display("failed to load web-auth session key"))]
    LoadSessionKey { source: key::LoadIdentityError },

    #[snafu(display("failed during web authentication"))]
    Poll { source: web::WebAuthRecvError },

    #[snafu(display("failed to update delegation"))]
    UpdateDelegation {
        source: key::UpdateWebAuthDelegationError,
    },
}
