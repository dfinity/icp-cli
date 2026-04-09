use clap::Args;
use ic_agent::{Identity as _, export::Principal, identity::BasicIdentity};
use icp::{context::Context, identity::key};
use snafu::{ResultExt, Snafu};
use tracing::info;

use crate::operations::ii_poll;

/// Link an Internet Identity to a new identity
#[derive(Debug, Args)]
pub(crate) struct IiArgs {
    /// Name for the linked identity
    name: String,
}

pub(crate) async fn exec(ctx: &Context, args: &IiArgs) -> Result<(), IiError> {
    let secret_key = ic_ed25519::PrivateKey::generate();
    let identity_key = key::IdentityKey::Ed25519(secret_key.clone());
    let basic = BasicIdentity::from_raw_key(&secret_key.serialize_raw());
    let der_public_key = basic.public_key().expect("ed25519 always has a public key");

    let chain = ii_poll::poll_for_delegation(&der_public_key)
        .await
        .context(PollSnafu)?;

    let from_key = hex::decode(&chain.public_key).context(DecodeFromKeySnafu)?;
    let ii_principal = Principal::self_authenticating(&from_key);

    ctx.dirs
        .identity()?
        .with_write(async |dirs| {
            key::link_ii_identity(dirs, &args.name, identity_key, &chain, ii_principal)
        })
        .await?
        .context(LinkSnafu)?;

    info!("Identity `{}` linked to Internet Identity", args.name);

    Ok(())
}

#[derive(Debug, Snafu)]
pub(crate) enum IiError {
    #[snafu(display("failed during II authentication"))]
    Poll { source: ii_poll::IiPollError },

    #[snafu(display("invalid public key in delegation chain"))]
    DecodeFromKey { source: hex::FromHexError },

    #[snafu(transparent)]
    LockIdentityDir { source: icp::fs::lock::LockError },

    #[snafu(display("failed to link II identity"))]
    Link { source: key::LinkIiIdentityError },
}
