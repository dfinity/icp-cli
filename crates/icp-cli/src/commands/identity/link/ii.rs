use std::net::SocketAddr;

use axum::{
    Form, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect},
    routing::post,
};
use base64::engine::{Engine as _, general_purpose::URL_SAFE_NO_PAD};
use clap::Args;
use dialoguer::Password;
use elliptic_curve::zeroize::Zeroizing;
use ic_agent::{Identity as _, export::Principal, identity::BasicIdentity};
use icp::{
    context::Context,
    fs::read_to_string,
    identity::{
        delegation::DelegationChain,
        key::{self, validate_password},
        manifest::IdentityList,
    },
    prelude::*,
};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use tokio::{net::TcpListener, sync::oneshot};
use tracing::{info, warn};
use url::Url;

use crate::commands::identity::StorageMode;

/// Link an Internet Identity to a new identity
#[derive(Debug, Args)]
pub(crate) struct IiArgs {
    /// Name for the linked identity
    name: String,

    /// Host of the II login frontend (e.g. example.icp0.io or https://example.icp0.io)
    #[arg(long, default_value = DEFAULT_HOST, value_parser = parse_host)]
    host: Url,

    /// Where to store the session private key
    #[arg(long, value_enum, default_value_t)]
    storage: StorageMode,

    /// Read the storage password from a file instead of prompting (for --storage password)
    #[arg(long, value_name = "FILE")]
    storage_password_file: Option<PathBuf>,
}

fn parse_host(s: &str) -> Result<Url, String> {
    let with_scheme = if s.contains("://") {
        s.to_owned()
    } else {
        format!("https://{s}")
    };
    Url::parse(&with_scheme).map_err(|e| e.to_string())
}

pub(crate) async fn exec(ctx: &Context, args: &IiArgs) -> Result<(), IiError> {
    ctx.dirs
        .identity()?
        .with_read(async |dirs| -> Result<(), IiError> {
            let list = IdentityList::load_from(dirs).context(LoadIdentityListSnafu)?;
            ensure!(
                !list.identities.contains_key(&args.name),
                NameTakenSnafu { name: &args.name }
            );
            Ok(())
        })
        .await??;

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
                    .validate_with(|s: &String| validate_password(s))
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

    // Linking creates a brand-new identity, so there is no principal to match against.
    let chain = recv_delegation(&args.host, &der_public_key, None)
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
    #[snafu(display("identity `{name}` already exists"))]
    NameTaken {
        name: String,
    },

    #[snafu(display("failed to load identity list"))]
    LoadIdentityList {
        source: icp::identity::manifest::LoadIdentityManifestError,
    },

    #[snafu(display("failed to read storage password file"))]
    ReadStoragePasswordFile {
        source: icp::fs::IoError,
    },

    #[snafu(display("failed to read storage password from terminal"))]
    StoragePasswordTermRead {
        source: dialoguer::Error,
    },

    #[snafu(display("failed during II authentication"))]
    Poll {
        source: IiRecvError,
    },

    #[snafu(display("invalid public key in delegation chain"))]
    DecodeFromKey {
        source: hex::FromHexError,
    },

    #[snafu(transparent)]
    LockIdentityDir {
        source: icp::fs::lock::LockError,
    },

    #[snafu(display("failed to link II identity"))]
    Link {
        source: key::CreatePendingDelegationError,
    },

    BadPassword {},
}

pub(crate) const DEFAULT_HOST: &str = "https://cli.id.ai";

#[derive(Debug, Snafu)]
pub(crate) enum IiRecvError {
    #[snafu(display("failed to bind local callback server"))]
    BindServer { source: std::io::Error },

    #[snafu(display("failed to run local callback server"))]
    ServeServer { source: std::io::Error },

    #[snafu(display("failed to fetch `{url}`"))]
    FetchDiscovery { url: String, source: reqwest::Error },

    #[snafu(display("failed to parse discovery response from `{url}` as JSON"))]
    ParseDiscovery { url: String, source: reqwest::Error },

    #[snafu(display("`{url}` returned an empty `path` — it must be a non-empty login path"))]
    EmptyLoginPath { url: String },

    #[snafu(display("interrupted"))]
    Interrupted,

    #[snafu(display("failed to open browser for II login"))]
    OpenBrowser { source: std::io::Error },

    #[snafu(display("failed to read confirmation from terminal"))]
    TermConfirm,

    #[snafu(display("the browser sent back an invalid delegation chain"))]
    InvalidDelegation,
}

/// Discovers the login path from the `{ "path": "…" }` JSON served at
/// `{host}/.well-known/cli-auth-config`, then opens a local HTTP server, builds
/// the login URL, and returns the delegation chain once the frontend submits it.
///
/// When `expected_principal` is set (the `login` re-auth path), a delegation
/// whose self-authenticating principal does not match is rejected without
/// stopping the server, so the frontend can show a mismatch message and retry.
pub(crate) async fn recv_delegation(
    host: &Url,
    der_public_key: &[u8],
    expected_principal: Option<Principal>,
) -> Result<DelegationChain, IiRecvError> {
    let key_b64 = URL_SAFE_NO_PAD.encode(der_public_key);

    // Discover the login path.
    let discovery_url = host
        .join("/.well-known/cli-auth-config")
        .expect("joining an absolute path is infallible");
    let discovery_url_str = discovery_url.to_string();
    let config: CliAuthConfig = reqwest::get(discovery_url)
        .await
        .context(FetchDiscoverySnafu {
            url: &discovery_url_str,
        })?
        .json()
        .await
        .context(ParseDiscoverySnafu {
            url: &discovery_url_str,
        })?;
    let login_path = config.path.trim();
    if login_path.is_empty() {
        return EmptyLoginPathSnafu {
            url: discovery_url_str,
        }
        .fail();
    }

    // Bind on a random port before opening the browser so the callback URL is known.
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context(BindServerSnafu)?;
    let addr: SocketAddr = listener.local_addr().context(BindServerSnafu)?;
    let callback_url = format!("http://127.0.0.1:{}/", addr.port());

    // Build the fragment as a URLSearchParams-compatible string so the frontend
    // can parse it with `new URLSearchParams(location.hash.slice(1))`.
    let fragment = {
        let mut scratch = Url::parse("x:?").expect("infallible");
        scratch
            .query_pairs_mut()
            .append_pair("public_key", &key_b64)
            .append_pair("callback", &callback_url);
        scratch.query().expect("just set").to_owned()
    };
    let mut login_url = host.join(login_path).expect("login_path is a valid path");
    login_url.set_fragment(Some(&fragment));

    // Where the loopback server sends the browser back to once it has the
    // delegation, so the frontend keeps ownership of the success/error UI. The
    // mismatch URL re-supplies `public_key`/`callback` so the user can pick
    // another identity and submit again to the same loopback server.
    let status_url = |status: &str| {
        let mut url = host.join(login_path).expect("login_path is a valid path");
        url.set_fragment(Some(&format!("status={status}")));
        url.to_string()
    };
    let success_url = status_url("success");
    let error_url = status_url("error");
    let mismatch_url = {
        let mut url = host.join(login_path).expect("login_path is a valid path");
        url.set_fragment(Some(&format!("{fragment}&status=identity-mismatch")));
        url.to_string()
    };

    let (chain_tx, mut chain_rx) = oneshot::channel::<Result<DelegationChain, ()>>();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    // In-flight events the handler reports while still listening (e.g. a
    // mismatch that the user can retry), so the CLI isn't silent during retries.
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<CallbackEvent>();

    let allowed_origin =
        HeaderValue::from_str(&host.origin().ascii_serialization()).expect("URL origin is valid");

    // chain_tx is wrapped in an Option so the handler can take ownership.
    let state = CallbackState {
        chain_tx: std::sync::Mutex::new(Some(chain_tx)),
        shutdown_tx: std::sync::Mutex::new(Some(shutdown_tx)),
        event_tx,
        allowed_origin,
        expected_principal,
        success_url,
        error_url,
        mismatch_url,
    };

    let app = Router::new()
        .route("/", post(handle_callback))
        .with_state(std::sync::Arc::new(state));

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("valid template"),
    );

    eprintln!();
    eprintln!("  Press Enter to log in at {}", {
        let mut display = login_url.clone();
        display.set_fragment(None);
        display
    });
    // Detached thread for stdin — tokio's async stdin keeps the runtime alive on drop.
    let (enter_tx, mut enter_rx) = tokio::sync::mpsc::channel::<()>(1);
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
        let _ = enter_tx.blocking_send(());
    });
    enter_rx.recv().await.context(TermConfirmSnafu)?;

    let serve = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx.await;
    });
    open::that(login_url.as_str()).context(OpenBrowserSnafu)?;
    let mut serve_fut = std::pin::pin!(serve.into_future());
    let result = loop {
        tokio::select! {
            res = &mut serve_fut => {
                res.context(ServeServerSnafu)?;
                panic!("receiving server ended unexpectedly");
            }
            Some(event) = event_rx.recv() => match event {
                CallbackEvent::Mismatch => spinner.println(
                    "  That identity doesn't match the one linked to this identity. \
                     Choose the correct identity in the browser to try again.",
                ),
            },
            chain = &mut chain_rx => break chain,
        }
    };

    spinner.finish_and_clear();
    match result.expect("sender only dropped after sending") {
        Ok(chain) => Ok(chain),
        Err(()) => InvalidDelegationSnafu.fail(),
    }
}

/// An event reported by the callback handler while it keeps listening.
#[derive(Debug)]
enum CallbackEvent {
    /// A delegation arrived whose principal didn't match `expected_principal`.
    Mismatch,
}

/// Response body of `{host}/.well-known/cli-auth-config`.
#[derive(Debug, Deserialize)]
struct CliAuthConfig {
    path: String,
}

/// Form fields posted by the frontend to the loopback callback.
#[derive(Debug, Deserialize)]
struct CallbackForm {
    /// The delegation chain as JSON (`DelegationChain.toJSON()`).
    delegation: String,
}

#[derive(Debug)]
struct CallbackState {
    chain_tx: std::sync::Mutex<Option<oneshot::Sender<Result<DelegationChain, ()>>>>,
    shutdown_tx: std::sync::Mutex<Option<oneshot::Sender<()>>>,
    event_tx: tokio::sync::mpsc::UnboundedSender<CallbackEvent>,
    allowed_origin: HeaderValue,
    /// When set, a delegation must resolve to this principal or it is rejected.
    expected_principal: Option<Principal>,
    success_url: String,
    error_url: String,
    mismatch_url: String,
}

/// Hands the result to `recv_delegation` and stops the server.
fn finish(state: &CallbackState, result: Result<DelegationChain, ()>) {
    if let Some(tx) = state.chain_tx.lock().unwrap().take() {
        let _ = tx.send(result);
    }
    if let Some(tx) = state.shutdown_tx.lock().unwrap().take() {
        let _ = tx.send(());
    }
}

/// The self-authenticating principal of the chain's leaf public key.
fn principal_of_chain(chain: &DelegationChain) -> Option<Principal> {
    let from_key = hex::decode(&chain.public_key).ok()?;
    Some(Principal::self_authenticating(&from_key))
}

async fn handle_callback(
    State(state): State<std::sync::Arc<CallbackState>>,
    headers: HeaderMap,
    Form(form): Form<CallbackForm>,
) -> axum::response::Response {
    // A top-level form-POST navigation from the II origin carries `Origin`.
    // Reject anything else without ending the flow, so stray or forged local
    // requests can't abort an in-progress login.
    let origin_ok = headers
        .get(header::ORIGIN)
        .map(|v| *v == state.allowed_origin)
        .unwrap_or(false);
    if !origin_ok {
        return StatusCode::FORBIDDEN.into_response();
    }

    let chain: DelegationChain = match serde_json::from_str(&form.delegation) {
        Ok(c) => c,
        Err(_) => {
            finish(&state, Err(()));
            return Redirect::to(&state.error_url).into_response();
        }
    };

    if let Some(expected) = state.expected_principal {
        match principal_of_chain(&chain) {
            // Keep listening so the user can retry with the right identity.
            Some(principal) if principal != expected => {
                let _ = state.event_tx.send(CallbackEvent::Mismatch);
                return Redirect::to(&state.mismatch_url).into_response();
            }
            Some(_) => {}
            None => {
                finish(&state, Err(()));
                return Redirect::to(&state.error_url).into_response();
            }
        }
    }

    finish(&state, Ok(chain));
    Redirect::to(&state.success_url).into_response()
}
