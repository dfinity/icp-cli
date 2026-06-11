use std::{io::IsTerminal, net::SocketAddr, time::Duration};

use anstyle::{AnsiColor, Reset, Style};
use axum::{
    Form, Router,
    extract::State,
    http::StatusCode,
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
use itertools::Itertools;
use rand::RngExt as _;
use serde::Deserialize;
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use tokio::{net::TcpListener, sync::oneshot};
use tracing::{info, warn};
use url::Url;

use crate::commands::identity::StorageMode;

/// Link a web-based identity (such as Internet Identity) to a new icp-cli identity
#[derive(Debug, Args)]
pub(crate) struct WebArgs {
    /// Name for the linked identity
    name: String,

    /// Auth domain to sign in at (e.g. id.ai or identity.ce1.com). Its
    /// `/.well-known/cli-auth-config` decides the login path.
    #[arg(long, default_value = DEFAULT_AUTH, value_parser = parse_auth)]
    auth: Url,

    /// Delegation domain to get an identity for (e.g. oisy.com). When omitted,
    /// the auth domain picks its default (id.ai uses cli.id.ai).
    #[arg(long)]
    app: Option<String>,

    /// Where to store the session private key
    #[arg(long, value_enum, default_value_t)]
    storage: StorageMode,

    /// Read the storage password from a file instead of prompting (for --storage password)
    #[arg(long, value_name = "FILE")]
    storage_password_file: Option<PathBuf>,
}

fn parse_auth(s: &str) -> Result<Url, String> {
    let with_scheme = if s.contains("://") {
        s.to_owned()
    } else {
        format!("https://{s}")
    };
    Url::parse(&with_scheme).map_err(|e| e.to_string())
}

pub(crate) async fn exec(ctx: &Context, args: &WebArgs) -> Result<(), WebAuthError> {
    ctx.dirs
        .identity()?
        .with_read(async |dirs| -> Result<(), WebAuthError> {
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
    let origin = args.app.as_deref().map(guesstimate_origin);

    // Linking creates a brand-new identity, so there is no principal to match against.
    let chain = recv_delegation(&args.auth, origin.as_deref(), &der_public_key, None)
        .await
        .context(PollSnafu)?;

    let from_key = hex::decode(&chain.public_key).context(DecodeFromKeySnafu)?;
    let remote_principal = Principal::self_authenticating(&from_key);

    let auth = args.auth.clone();
    ctx.dirs
        .identity()?
        .with_write(async |dirs| {
            key::link_webauth_identity(
                dirs,
                &args.name,
                identity_key,
                &chain,
                remote_principal,
                create_format,
                auth,
                origin,
            )
        })
        .await?
        .context(LinkSnafu)?;

    info!("Identity `{}` linked from web identity", args.name);

    if matches!(args.storage, StorageMode::Plaintext) {
        warn!(
            "This identity is stored in plaintext and is not secure. Do not use it for anything of significant value."
        );
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub(crate) enum WebAuthError {
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

    #[snafu(display("failed during web authentication"))]
    Poll {
        source: WebAuthRecvError,
    },

    #[snafu(display("invalid public key in delegation chain"))]
    DecodeFromKey {
        source: hex::FromHexError,
    },

    #[snafu(transparent)]
    LockIdentityDir {
        source: icp::fs::lock::LockError,
    },

    #[snafu(display("failed to link web-auth identity"))]
    Link {
        source: key::CreatePendingDelegationError,
    },

    BadPassword {},
}

pub(crate) const DEFAULT_AUTH: &str = "https://id.ai";

#[derive(Debug, Snafu)]
pub(crate) enum WebAuthRecvError {
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

    #[snafu(display("failed to open browser for login"))]
    OpenBrowser { source: std::io::Error },

    #[snafu(display("failed to read confirmation from terminal"))]
    TermConfirm,

    #[snafu(display("the browser sent back an invalid delegation chain"))]
    InvalidDelegation,
}

/// Discovers the login path from the `{ "path": "…" }` JSON served at
/// `{auth}/.well-known/cli-auth-config`, then opens a local HTTP server, builds
/// the login URL, and returns the delegation chain once the frontend submits it.
///
/// `delegation_domain` is the domain to get a delegation for; when `None`, the
/// auth page picks its own default (id.ai uses cli.id.ai).
///
/// When `expected_principal` is set (the `login` re-auth path), a delegation
/// whose self-authenticating principal does not match is rejected without
/// stopping the server, so the frontend can show a mismatch message and retry.
pub(crate) async fn recv_delegation(
    auth: &Url,
    delegation_domain: Option<&str>,
    der_public_key: &[u8],
    expected_principal: Option<Principal>,
) -> Result<DelegationChain, WebAuthRecvError> {
    let key_b64 = URL_SAFE_NO_PAD.encode(der_public_key);

    // Single-use secret shared with the frontend via the URL fragment, which is
    // never sent over the network and is unreadable cross-origin. The frontend
    // echoes it back in its POST, proving the request came from the page the
    // user logged in through rather than a stray or forged local request.
    let nonce_bytes: [u8; 32] = rand::rng().random();
    let nonce = URL_SAFE_NO_PAD.encode(nonce_bytes);

    // Discover the login path.
    let discovery_url = auth
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
        {
            let mut pairs = scratch.query_pairs_mut();
            pairs
                .append_pair("public_key", &key_b64)
                .append_pair("callback", &callback_url)
                .append_pair("nonce", &nonce);
            // Omitted when the caller passes no `--app`, so the auth page picks
            // its own default delegation domain.
            if let Some(domain) = delegation_domain {
                pairs.append_pair("domain", domain);
            }
        }
        scratch.query().expect("just set").to_owned()
    };
    let mut login_url = auth.join(login_path).expect("login_path is a valid path");
    login_url.set_fragment(Some(&fragment));

    // Where the loopback server sends the browser back to once it has the
    // delegation, so the frontend keeps ownership of the success/error UI. The
    // mismatch URL re-supplies the request params (`public_key`/`callback`/
    // `nonce`/`domain`) so the user can pick another identity and submit again
    // to the same loopback server.
    let status_url = |status: &str| {
        let mut url = auth.join(login_path).expect("login_path is a valid path");
        url.set_fragment(Some(&format!("status={status}")));
        url.to_string()
    };
    let success_url = status_url("success");
    let error_url = status_url("error");
    let mismatch_url = {
        let mut url = auth.join(login_path).expect("login_path is a valid path");
        url.set_fragment(Some(&format!("{fragment}&status=identity-mismatch")));
        url.to_string()
    };

    let (chain_tx, mut chain_rx) = oneshot::channel::<Result<DelegationChain, ()>>();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    // In-flight events the handler reports while still listening (e.g. a
    // mismatch that the user can retry), so the CLI isn't silent during retries.
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<CallbackEvent>();

    // chain_tx is wrapped in an Option so the handler can take ownership.
    let state = CallbackState {
        chain_tx: std::sync::Mutex::new(Some(chain_tx)),
        shutdown_tx: std::sync::Mutex::new(Some(shutdown_tx)),
        event_tx,
        expected_nonce: nonce,
        expected_principal,
        success_url,
        error_url,
        mismatch_url,
    };

    let app = Router::new()
        .route("/", post(handle_callback))
        .with_state(std::sync::Arc::new(state));

    // Animated braille frames while waiting, with a green `✓` as the final
    // tick so a finished step matches the checkmark on the id.ai/cli screen.
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::with_template("{spinner:.green} {msg}")
            .expect("valid template")
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "✓"]),
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
    // Mirror the id.ai/cli terminal block: a checked "Browser opened" line, then
    // an animated "Linking web-based identity" step until the delegation lands.
    spinner.println(format!("{} Browser opened", green_check()));
    spinner.set_message("Linking web-based identity");
    spinner.enable_steady_tick(Duration::from_millis(80));
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

    match result.expect("sender only dropped after sending") {
        Ok(chain) => {
            // Leaves a checked "✓ Linking web-based identity" line (the final
            // tick of the spinner style).
            spinner.finish_with_message("Linking web-based identity");
            Ok(chain)
        }
        Err(()) => {
            spinner.finish_and_clear();
            InvalidDelegationSnafu.fail()
        }
    }
}

/// A green `✓`, or a plain one when color is disabled or stderr isn't a TTY —
/// matching how the rest of the CLI gates ANSI styling.
fn green_check() -> String {
    const GREEN: Style = AnsiColor::Green.on_default();
    if std::env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal() {
        format!("{GREEN}✓{Reset}")
    } else {
        "✓".to_string()
    }
}

/// An event reported by the callback handler while it keeps listening.
#[derive(Debug)]
enum CallbackEvent {
    /// A delegation arrived whose principal didn't match `expected_principal`.
    Mismatch,
}

/// Response body of `{auth}/.well-known/cli-auth-config`.
#[derive(Debug, Deserialize)]
struct CliAuthConfig {
    path: String,
}

/// Form fields posted by the frontend to the loopback callback.
#[derive(Debug, Deserialize)]
struct CallbackForm {
    /// The delegation chain as JSON (`DelegationChain.toJSON()`).
    delegation: String,
    /// The single-use nonce echoed back from the login URL fragment.
    nonce: String,
}

#[derive(Debug)]
struct CallbackState {
    chain_tx: std::sync::Mutex<Option<oneshot::Sender<Result<DelegationChain, ()>>>>,
    shutdown_tx: std::sync::Mutex<Option<oneshot::Sender<()>>>,
    event_tx: tokio::sync::mpsc::UnboundedSender<CallbackEvent>,
    /// The nonce the frontend must echo back, proving it read the login URL fragment.
    expected_nonce: String,
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
    Form(form): Form<CallbackForm>,
) -> axum::response::Response {
    // Only the frontend the user logged in through saw the nonce in the login
    // URL fragment. Reject anything else without ending the flow, so stray or
    // forged local requests can't abort an in-progress login.
    if form.nonce != state.expected_nonce {
        warn!("rejecting callback POST: nonce missing or mismatched");
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

fn guesstimate_origin(origin: &str) -> String {
    let hierarchy = origin.split_once("://").map_or(origin, |(_, rest)| rest);
    let origin = hierarchy.split('/').next().unwrap_or(origin);
    if origin == "nns.internetcomputer.org" {
        // If an app uses alternativeOrigins, (a) that's the required domain, and (b) there's no way to know what it is at the time of writing.
        // Temporary hack: NNS is the most common app that would break. Special-case it
        return "nns.ic0.app".to_string();
    }
    // Rewrite <principal>.icp0.io and <principal>.icp.net to <principal>.ic0.app, since that's what Internet Identity uses for them.
    // May be done automatically by II in the future.
    let parts: Vec<_> = origin.split('.').collect();
    if parts.len() <= 2 {
        return origin.to_string();
    }
    let (stem, root) = parts.split_at(parts.len() - 2);
    if (root == ["icp0", "io"] || root == ["icp", "net"])
        && (stem.len() == 1 || (stem.len() == 2 && stem[1] == "raw"))
        && Principal::from_text(stem[0]).is_ok()
    {
        stem.iter().chain(&["ic0", "app"]).format(".").to_string()
    } else {
        origin.to_string()
    }
}
