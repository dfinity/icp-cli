use base64::engine::{Engine as _, general_purpose::URL_SAFE_NO_PAD};
use clap::Args;
use ic_agent::{Identity as _, export::Principal, identity::BasicIdentity};
use icp::{
    context::Context,
    identity::{key, delegation::DelegationChain},
    signal,
};
use indicatif::{ProgressBar, ProgressStyle};
use snafu::{ResultExt, Snafu};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::info;

pub(crate) const DEFAULT_LOGIN_HOST: &str = "frontend.local.localhost:8000";

/// Link an Internet Identity to a new identity
#[derive(Debug, Args)]
pub(crate) struct IiArgs {
    /// Name for the linked identity
    name: String,

    /// Login frontend host (domain:port)
    #[arg(long, default_value = DEFAULT_LOGIN_HOST)]
    login_host: String,
}

pub(crate) async fn exec(ctx: &Context, args: &IiArgs) -> Result<(), IiError> {
    // Generate an Ed25519 keypair for the session key
    let secret_key = ic_ed25519::PrivateKey::generate();
    let identity_key = key::IdentityKey::Ed25519(secret_key.clone());
    let basic = BasicIdentity::from_raw_key(&secret_key.serialize_raw());
    let der_public_key = basic.public_key().expect("ed25519 always has a public key");
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

    // Derive the II principal from the root of the delegation chain
    let from_key = hex::decode(&chain.public_key).context(DecodeFromKeySnafu)?;
    let ii_principal = Principal::self_authenticating(&from_key);

    // Save the identity
    ctx.dirs
        .identity()?
        .with_write(async |dirs| {
            key::link_ii_identity(dirs, &args.name, identity_key, &chain, ii_principal, &args.login_host)
        })
        .await?
        .context(LinkSnafu)?;

    info!("Identity `{}` linked to Internet Identity", args.name);

    Ok(())
}

// ---------------------------------------------------------------------------
// Callback server – listens for the II frontend to POST a delegation chain
// ---------------------------------------------------------------------------

/// A local HTTP server that listens for the II frontend to POST the delegation
/// chain back to the CLI.
pub(crate) struct CallbackServer {
    listener: TcpListener,
    pub port: u16,
    allowed_origin: String,
}

impl CallbackServer {
    /// Bind a TCP listener on `127.0.0.1` with a random available port.
    /// `login_host` is used to restrict CORS to the login frontend origin.
    pub async fn bind(login_host: &str) -> Result<Self, CallbackError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context(BindSnafu)?;
        let port = listener.local_addr().context(LocalAddrSnafu)?.port();
        let allowed_origin = format!("http://{login_host}");
        Ok(Self {
            listener,
            port,
            allowed_origin,
        })
    }

    /// Accept connections until we receive a POST /callback with a valid
    /// delegation chain. OPTIONS preflight requests are handled automatically.
    pub async fn wait_for_delegation(self) -> Result<DelegationChain, CallbackError> {
        loop {
            let (stream, _) = self.listener.accept().await.context(AcceptSnafu)?;
            if let Some(chain) = handle_connection(stream, &self.allowed_origin).await? {
                return Ok(chain);
            }
        }
    }
}

fn cors_headers(allowed_origin: &str) -> String {
    format!(
        "Access-Control-Allow-Origin: {allowed_origin}\r\n\
         Access-Control-Allow-Methods: POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n"
    )
}

async fn handle_connection(
    mut stream: TcpStream,
    allowed_origin: &str,
) -> Result<Option<DelegationChain>, CallbackError> {
    // Read until we have the full headers (terminated by \r\n\r\n).
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut tmp).await.context(ReadSnafu)?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
    };

    let headers = String::from_utf8_lossy(&buf[..header_end]);
    let first_line = headers.lines().next().unwrap_or("");

    let cors = cors_headers(allowed_origin);

    // CORS preflight
    if first_line.starts_with("OPTIONS") {
        let resp = format!("HTTP/1.1 204 No Content\r\n{cors}\r\n");
        stream.write_all(resp.as_bytes()).await.context(WriteSnafu)?;
        return Ok(None);
    }

    // Only accept POST /callback
    if !first_line.starts_with("POST") || !first_line.contains("/callback") {
        stream
            .write_all(b"HTTP/1.1 404 Not Found\r\n\r\n")
            .await
            .context(WriteSnafu)?;
        return Ok(None);
    }

    // Parse Content-Length
    let content_length: usize = headers
        .lines()
        .find_map(|line| {
            let lower = line.to_lowercase();
            if lower.starts_with("content-length:") {
                line[15..].trim().parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    // Read the body (some bytes may already be in the buffer past the headers).
    let body_start = header_end + 4;
    let mut body = buf[body_start..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut tmp).await.context(ReadSnafu)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(content_length);

    // Respond to the browser before parsing so it knows we received the data.
    let resp = format!(
        "HTTP/1.1 200 OK\r\n{cors}Content-Type: text/plain\r\nContent-Length: 2\r\n\r\nOK"
    );
    stream.write_all(resp.as_bytes()).await.context(WriteSnafu)?;

    let chain: DelegationChain =
        serde_json::from_slice(&body).context(ParseDelegationSnafu)?;
    Ok(Some(chain))
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Snafu)]
pub(crate) enum IiError {
    #[snafu(display("callback server error"))]
    Callback { source: CallbackError },

    #[snafu(display("interrupted"))]
    Interrupted,

    #[snafu(display("invalid public key in delegation chain"))]
    DecodeFromKey { source: hex::FromHexError },

    #[snafu(transparent)]
    LockIdentityDir { source: icp::fs::lock::LockError },

    #[snafu(display("failed to link II identity"))]
    Link { source: key::LinkIiIdentityError },
}

#[derive(Debug, Snafu)]
pub(crate) enum CallbackError {
    #[snafu(display("failed to bind callback server"))]
    Bind { source: std::io::Error },

    #[snafu(display("failed to get local address of callback server"))]
    LocalAddr { source: std::io::Error },

    #[snafu(display("failed to accept connection on callback server"))]
    Accept { source: std::io::Error },

    #[snafu(display("failed to read from connection"))]
    Read { source: std::io::Error },

    #[snafu(display("failed to write to connection"))]
    Write { source: std::io::Error },

    #[snafu(display("failed to parse delegation chain from callback body"))]
    ParseDelegation { source: serde_json::Error },
}
