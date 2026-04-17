use std::net::SocketAddr;

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::IntoResponse,
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
    identity::{delegation::DelegationChain, key},
    prelude::*,
    signal,
};
use indicatif::{ProgressBar, ProgressStyle};
use snafu::{ResultExt, Snafu};
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

    let chain = recv_delegation(&args.host, &der_public_key)
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
    Poll { source: IiRecvError },

    #[snafu(display("invalid public key in delegation chain"))]
    DecodeFromKey { source: hex::FromHexError },

    #[snafu(transparent)]
    LockIdentityDir { source: icp::fs::lock::LockError },

    #[snafu(display("failed to link II identity"))]
    Link { source: key::LinkIiIdentityError },
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

    #[snafu(display("failed to read discovery response from `{url}`"))]
    ReadDiscovery { url: String, source: reqwest::Error },

    #[snafu(display(
        "`{url}` returned an empty login path — the response must be a single non-empty line"
    ))]
    EmptyLoginPath { url: String },

    #[snafu(display("interrupted"))]
    Interrupted,
}

/// Discovers the login path from `{host}/.well-known/ic-cli-login`, then opens
/// a local HTTP server, builds the login URL, and returns the delegation chain
/// once the frontend POSTs it back.
pub(crate) async fn recv_delegation(
    host: &Url,
    der_public_key: &[u8],
) -> Result<DelegationChain, IiRecvError> {
    let key_b64 = URL_SAFE_NO_PAD.encode(der_public_key);

    // Discover the login path.
    let discovery_url = host
        .join("/.well-known/ic-cli-login")
        .expect("joining an absolute path is infallible");
    let discovery_url_str = discovery_url.to_string();
    let login_path = reqwest::get(discovery_url)
        .await
        .context(FetchDiscoverySnafu {
            url: &discovery_url_str,
        })?
        .text()
        .await
        .context(ReadDiscoverySnafu {
            url: &discovery_url_str,
        })?;
    let login_path = login_path.trim();
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

    eprintln!();
    eprintln!("  Press Enter to log in at {}", {
        let mut display = login_url.clone();
        display.set_fragment(None);
        display
    });

    let (chain_tx, chain_rx) = oneshot::channel::<DelegationChain>();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // chain_tx is wrapped in an Option so the handler can take ownership.
    let state = CallbackState {
        chain_tx: std::sync::Mutex::new(Some(chain_tx)),
        shutdown_tx: std::sync::Mutex::new(Some(shutdown_tx)),
    };

    let app = Router::new()
        .route("/", post(handle_callback).options(handle_preflight))
        .with_state(std::sync::Arc::new(state));

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("valid template"),
    );

    // Detached thread for stdin — tokio's async stdin keeps the runtime alive on drop.
    let (enter_tx, mut enter_rx) = tokio::sync::mpsc::channel::<()>(1);
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
        let _ = enter_tx.blocking_send(());
    });

    let serve = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = shutdown_rx.await;
    });

    let mut browser_opened = false;

    let result = tokio::select! {
        _ = signal::stop_signal() => {
            spinner.finish_and_clear();
            return InterruptedSnafu.fail();
        }
        res = serve.into_future() => {
            res.context(ServeServerSnafu)?;
            // Server shut down before we got a chain — shouldn't happen.
            return InterruptedSnafu.fail();
        }
        _ = async {
            loop {
                tokio::select! {
                    _ = enter_rx.recv(), if !browser_opened => {
                        browser_opened = true;
                        spinner.set_message("Waiting for Internet Identity authentication...");
                        spinner.enable_steady_tick(std::time::Duration::from_millis(100));
                        let _ = open::that(login_url.as_str());
                    }
                    // Yield so the other branches in the outer select! can fire.
                    _ = tokio::task::yield_now() => {}
                }
            }
        } => { unreachable!() }
        chain = chain_rx => chain,
    };

    spinner.finish_and_clear();
    Ok(result.expect("sender only dropped after sending"))
}

#[derive(Debug)]
struct CallbackState {
    chain_tx: std::sync::Mutex<Option<oneshot::Sender<DelegationChain>>>,
    shutdown_tx: std::sync::Mutex<Option<oneshot::Sender<()>>>,
}

fn cors_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("content-type"),
    );
    headers
}

async fn handle_preflight() -> impl IntoResponse {
    (StatusCode::NO_CONTENT, cors_headers())
}

async fn handle_callback(
    State(state): State<std::sync::Arc<CallbackState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    // Only accept POST with JSON content.
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type.starts_with("application/json") {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            cors_headers(),
            "expected application/json",
        )
            .into_response();
    }

    let chain: DelegationChain = match serde_json::from_slice(&body) {
        Ok(c) => c,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                cors_headers(),
                "invalid delegation chain",
            )
                .into_response();
        }
    };

    if let Some(tx) = state.chain_tx.lock().unwrap().take() {
        let _ = tx.send(chain);
    }
    if let Some(tx) = state.shutdown_tx.lock().unwrap().take() {
        let _ = tx.send(());
    }

    (StatusCode::OK, cors_headers(), "").into_response()
}
