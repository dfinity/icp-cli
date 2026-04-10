use clap::Args;
use dialoguer::Password;
use elliptic_curve::zeroize::Zeroizing;
use ic_agent::{Identity as _, export::Principal, identity::BasicIdentity};
use icp::{context::Context, fs::read_to_string, identity::key, prelude::*};
use snafu::{ResultExt, Snafu};
use tracing::{info, warn};
use url::Url;

use crate::{commands::identity::StorageMode, operations::ii_poll};

/// Link an Internet Identity to a new identity
#[derive(Debug, Args)]
pub(crate) struct IiArgs {
    /// Name for the linked identity
    name: String,

    /// Host of the II login frontend (e.g. https://example.icp0.io)
    #[arg(long, default_value = ii_poll::DEFAULT_HOST)]
    host: Url,

    /// Where to store the session private key
    #[arg(long, value_enum, default_value_t)]
    storage: StorageMode,

    /// Read the storage password from a file instead of prompting (for --storage password)
    #[arg(long, value_name = "FILE")]
    storage_password_file: Option<PathBuf>,
}

pub(crate) async fn exec(ctx: &Context, args: &IiArgs) -> Result<(), IiError> {
    let create_format = match args.storage {
        StorageMode::Plaintext => key::CreateFormat::Plaintext,
        StorageMode::Keyring => key::CreateFormat::Keyring,
        StorageMode::Password => {
            let password = if let Some(path) = &args.storage_password_file {
                read_to_string(path)
                    .context(ReadStoragePasswordFileSnafu)?
                    .trim()
                    .to_string()
            } else {
                Password::new()
                    .with_prompt("Enter password to encrypt identity")
                    .with_confirmation("Confirm password", "Passwords do not match")
                    .interact()
                    .context(StoragePasswordTermReadSnafu)?
            };
            key::CreateFormat::Pbes2 {
                password: Zeroizing::new(password),
            }
        }
    };

    let secret_key = ic_ed25519::PrivateKey::generate();
    let identity_key = key::IdentityKey::Ed25519(secret_key.clone());
    let basic = BasicIdentity::from_raw_key(&secret_key.serialize_raw());
    let der_public_key = basic.public_key().expect("ed25519 always has a public key");

    let chain = ii_poll::poll_for_delegation(&args.host, &der_public_key)
        .await
        .context(PollSnafu)?;

    let from_key = hex::decode(&chain.public_key).context(DecodeFromKeySnafu)?;
    let ii_principal = Principal::self_authenticating(&from_key);

    let host = args.host.clone();
    ctx.dirs
        .identity()?
        .with_write(async |dirs| {
            key::link_ii_identity(
                dirs,
                &args.name,
                identity_key,
                &chain,
                ii_principal,
                create_format,
                host,
            )
        })
        .await?
        .context(LinkSnafu)?;

    info!("Identity `{}` linked to Internet Identity", args.name);

    if matches!(args.storage, StorageMode::Plaintext) {
        warn!(
            "This identity is stored in plaintext and is not secure. Do not use it for anything of significant value."
        );
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub(crate) enum IiError {
    #[snafu(display("failed to read storage password file"))]
    ReadStoragePasswordFile { source: icp::fs::IoError },

    #[snafu(display("failed to read storage password from terminal"))]
    StoragePasswordTermRead { source: dialoguer::Error },

    #[snafu(display("failed during II authentication"))]
    Poll { source: ii_poll::IiPollError },

    #[snafu(display("invalid public key in delegation chain"))]
    DecodeFromKey { source: hex::FromHexError },

    #[snafu(transparent)]
    LockIdentityDir { source: icp::fs::lock::LockError },

    #[snafu(display("failed to link II identity"))]
    Link { source: key::LinkIiIdentityError },
}
