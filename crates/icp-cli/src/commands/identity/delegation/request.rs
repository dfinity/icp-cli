use clap::Args;
use dialoguer::Password;
use elliptic_curve::zeroize::Zeroizing;
use icp::{context::Context, fs::read_to_string, identity::key, prelude::*};
use pem::Pem;
use snafu::{ResultExt, Snafu};
use tracing::warn;

use crate::commands::identity::StorageMode;

/// Create a pending delegation identity with a new P256 session key
///
/// Prints the session public key as a PEM-encoded SPKI to stdout. Pass this to
/// `icp identity delegation sign --key-pem` on another machine to obtain a
/// delegation chain, then complete the identity with `icp identity delegation use`.
#[derive(Debug, Args)]
pub(crate) struct RequestArgs {
    /// Name for the new identity
    name: String,

    /// Where to store the session private key
    #[arg(long, value_enum, default_value_t)]
    storage: StorageMode,

    /// Read the storage password from a file instead of prompting (for --storage password)
    #[arg(long, value_name = "FILE")]
    storage_password_file: Option<PathBuf>,
}

pub(crate) async fn exec(ctx: &Context, args: &RequestArgs) -> Result<(), RequestError> {
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

    let der_public_key = ctx
        .dirs
        .identity()?
        .with_write(async |dirs| key::create_pending_delegation(dirs, &args.name, create_format))
        .await?
        .context(CreateSnafu)?;

    let pem = pem::encode(&Pem::new("PUBLIC KEY", der_public_key));
    print!("{pem}");

    if matches!(args.storage, StorageMode::Plaintext) {
        warn!(
            "This identity is stored in plaintext and is not secure. Do not use it for anything of significant value."
        );
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub(crate) enum RequestError {
    #[snafu(display("failed to read storage password file"))]
    ReadStoragePasswordFile { source: icp::fs::IoError },

    #[snafu(display("failed to read storage password from terminal"))]
    StoragePasswordTermRead { source: dialoguer::Error },

    #[snafu(transparent)]
    LockIdentityDir { source: icp::fs::lock::LockError },

    #[snafu(display("failed to create pending delegation identity"))]
    Create {
        source: key::CreatePendingDelegationError,
    },
}
