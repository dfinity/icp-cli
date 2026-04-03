use base64::engine::{Engine as _, general_purpose::URL_SAFE_NO_PAD};
use clap::Args;
use ic_agent::Identity as _;
use icp::{
    context::Context,
    identity::{
        key,
        manifest::{IdentityKeyAlgorithm, IdentityList, IdentitySpec},
    },
    signal,
};
use pem::Pem;
use pkcs8::DecodePrivateKey as _;
use sec1::pem::PemLabel as _;
use indicatif::{ProgressBar, ProgressStyle};
use snafu::{OptionExt, ResultExt, Snafu};
use std::time::Duration;
use tracing::info;

use crate::commands::identity::link::ii::{CallbackServer, DEFAULT_LOGIN_HOST};

/// Re-authenticate an Internet Identity delegation
#[derive(Debug, Args)]
pub(crate) struct LoginArgs {
    /// Name of the identity to re-authenticate
    name: String,

    /// Login frontend host (domain:port)
    #[arg(long, default_value = DEFAULT_LOGIN_HOST)]
    login_host: String,
}

pub(crate) async fn exec(ctx: &Context, args: &LoginArgs) -> Result<(), LoginError> {
    // Load the identity list and verify this is an II identity
    let der_public_key =
        ctx.dirs
            .identity()?
            .with_read(async |dirs| {
                let list = IdentityList::load_from(dirs)?;
                let spec = list
                    .identities
                    .get(&args.name)
                    .context(IdentityNotFoundSnafu { name: &args.name })?;

                let algorithm = match spec {
                    IdentitySpec::InternetIdentity { algorithm, .. } => algorithm.clone(),
                    _ => return NotIiSnafu { name: &args.name }.fail(),
                };

                // Load the existing PEM to get the public key
                let pem_path = dirs.key_pem_path(&args.name);
                let origin = key::PemOrigin::File {
                    path: pem_path.clone(),
                };
                let doc = icp::fs::read_to_string(&pem_path)?
                    .parse::<Pem>()
                    .map_err(|e| LoginError::ParsePem {
                        origin: origin.clone(),
                        source: Box::new(e),
                    })?;

                assert!(
                    doc.tag() == pkcs8::PrivateKeyInfo::PEM_LABEL,
                    "II identity PEM should be plaintext"
                );

                let der_public_key = match algorithm {
                    IdentityKeyAlgorithm::Ed25519 => {
                        let key = ic_ed25519::PrivateKey::deserialize_pkcs8(doc.contents())
                            .map_err(|e| LoginError::ParseKey {
                                origin: origin.clone(),
                                source: Box::new(e),
                            })?;
                        let basic =
                            ic_agent::identity::BasicIdentity::from_raw_key(&key.serialize_raw());
                        basic.public_key().expect("ed25519 always has a public key")
                    }
                    IdentityKeyAlgorithm::Secp256k1 => {
                        let key = k256::SecretKey::from_pkcs8_der(doc.contents()).map_err(|e| {
                            LoginError::ParseKey {
                                origin: origin.clone(),
                                source: Box::new(e),
                            }
                        })?;
                        let id = ic_agent::identity::Secp256k1Identity::from_private_key(key);
                        id.public_key().expect("secp256k1 always has a public key")
                    }
                    IdentityKeyAlgorithm::Prime256v1 => {
                        let key = p256::SecretKey::from_pkcs8_der(doc.contents()).map_err(|e| {
                            LoginError::ParseKey {
                                origin: origin.clone(),
                                source: Box::new(e),
                            }
                        })?;
                        let id = ic_agent::identity::Prime256v1Identity::from_private_key(key);
                        id.public_key().expect("p256 always has a public key")
                    }
                };

                Ok(der_public_key)
            })
            .await??;

    let public_key_b64 = URL_SAFE_NO_PAD.encode(&der_public_key);

    // Start the local callback server on a random port
    let server = CallbackServer::bind(&args.login_host).await.context(CallbackSnafu)?;
    let callback_url = format!("http://127.0.0.1:{}/callback", server.port);

    // Build the login URL
    let login_url = format!(
        "http://{}/cli-login?public_key={public_key_b64}&callback={callback_url}",
        args.login_host
    );

    eprintln!();
    eprintln!("  Press Enter to open {login_url}");

    // Spawn a detached thread for stdin — using tokio would hang the runtime
    // because the blocking read never completes.
    let (enter_tx, mut enter_rx) = tokio::sync::mpsc::channel::<()>(1);
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
        let _ = enter_tx.blocking_send(());
    });

    // Wait for Enter or Ctrl-C, then open the browser and wait for the callback.
    let chain = loop {
        tokio::select! {
            _ = signal::stop_signal() => {
                return InterruptedSnafu.fail();
            }
            _ = enter_rx.recv() => {
                let _ = open::that(&login_url);
                let spinner = ProgressBar::new_spinner();
                spinner.set_style(
                    ProgressStyle::default_spinner()
                        .template("{spinner:.green} {msg}")
                        .expect("valid template"),
                );
                spinner.set_message("Waiting for Internet Identity authentication...");
                spinner.enable_steady_tick(Duration::from_millis(100));
                let result = server.wait_for_delegation().await.context(CallbackSnafu);
                spinner.finish_and_clear();
                break result?;
            }
        }
    };

    // Update the delegation chain
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

    #[snafu(transparent)]
    ReadFile { source: icp::fs::IoError },

    #[snafu(display("failed to parse PEM from `{origin}`"))]
    ParsePem {
        origin: key::PemOrigin,
        #[snafu(source(from(pem::PemError, Box::new)))]
        source: Box<pem::PemError>,
    },

    #[snafu(display("failed to parse key from `{origin}`"))]
    ParseKey {
        origin: key::PemOrigin,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[snafu(display("callback server error"))]
    Callback {
        source: crate::commands::identity::link::ii::CallbackError,
    },

    #[snafu(display("interrupted"))]
    Interrupted,

    #[snafu(display("failed to update delegation"))]
    UpdateDelegation {
        source: key::UpdateIiDelegationError,
    },
}
