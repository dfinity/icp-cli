use std::time::Duration;

use base64::engine::{Engine as _, general_purpose::URL_SAFE_NO_PAD};
use candid::{CandidType, Decode, Encode};
use ic_agent::{Agent, export::Principal};
use icp::{identity::delegation::DelegationChain, network::custom_domains, signal};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use snafu::{ResultExt, Snafu};
use url::Url;

/// Candid `RegisterResult` variant from the cli-backend canister.
#[derive(Debug, Clone, CandidType, Deserialize)]
pub(crate) enum RegisterResult {
    #[serde(rename = "ok")]
    Ok(String),
    #[serde(rename = "err")]
    Err(String),
}

#[derive(Debug, Snafu)]
pub(crate) enum IiPollError {
    #[snafu(display("failed to register session with cli-backend canister"))]
    Register { source: ic_agent::AgentError },

    #[snafu(display("cli-backend canister rejected registration: {message}"))]
    RegisterRejected { message: String },

    #[snafu(display("failed to query cli-backend canister"))]
    Query { source: ic_agent::AgentError },

    #[snafu(display("failed to decode candid response"))]
    CandidDecode { source: candid::Error },

    #[snafu(display("interrupted"))]
    Interrupted,
}

/// Registers a session with the cli-backend canister, prints a one-time code
/// for the user to enter on the login website, and polls until the delegation
/// chain is stored. Returns the received delegation chain.
pub(crate) async fn poll_for_delegation(
    agent: &Agent,
    delegator_backend_id: Principal,
    delegator_frontend_id: Principal,
    der_public_key: &[u8],
    http_gateway_url: &Url,
    delegator_frontend_friendly_name: Option<(&str, &str)>,
) -> Result<DelegationChain, IiPollError> {
    let uuid = uuid::Uuid::new_v4().to_string();
    let key_b64 = URL_SAFE_NO_PAD.encode(der_public_key);

    // Register the session with the backend canister
    let register_args = Encode!(&uuid, &key_b64).expect("infallible candid encode");
    let register_response = agent
        .update(&delegator_backend_id, "register")
        .with_arg(register_args)
        .call_and_wait()
        .await
        .context(RegisterSnafu)?;

    let result = Decode!(&register_response, RegisterResult).context(CandidDecodeSnafu)?;

    let code = match result {
        RegisterResult::Ok(code) => code,
        RegisterResult::Err(message) => return RegisterRejectedSnafu { message }.fail(),
    };

    // Construct the frontend login URL
    let mut login_url = custom_domains::canister_gateway_url(
        http_gateway_url,
        delegator_frontend_id,
        delegator_frontend_friendly_name,
    );
    login_url.set_path("/cli-login");

    eprintln!();
    eprintln!("  Your one-time code is:");
    eprintln!();
    eprintln!("    {code}");
    eprintln!();
    eprintln!("  Press Enter to open {login_url}");

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("valid template"),
    );

    let poll_args = Encode!(&uuid).expect("infallible candid encode");

    // Spawn a detached thread for stdin.
    // If this is done with tokio instead, because the read never completes, the runtime hangs.
    let (enter_tx, mut enter_rx) = tokio::sync::mpsc::channel::<()>(1);
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = std::io::stdin().read_line(&mut buf);
        let _ = enter_tx.blocking_send(());
    });

    let mut browser_opened = false;
    let mut fuse = false;

    loop {
        if browser_opened && !fuse {
            spinner.set_message("Waiting for Internet Identity authentication...");
            spinner.enable_steady_tick(Duration::from_millis(100));
            fuse = true;
        }

        tokio::select! {
            _ = signal::stop_signal() => {
                spinner.finish_and_clear();
                return InterruptedSnafu.fail();
            }
            _ = enter_rx.recv(), if !browser_opened => {
                browser_opened = true;
                let _ = open::that(login_url.as_str());
            }
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                let response = agent
                    .query(&delegator_backend_id, "get_delegation")
                    .with_arg(poll_args.clone())
                    .call()
                    .await
                    .context(QuerySnafu)?;

                let chain = Decode!(&response, Option<DelegationChain>)
                    .context(CandidDecodeSnafu)?;

                if let Some(chain) = chain {
                    spinner.finish_and_clear();
                    return Ok(chain);
                }
            }
        }
    }
}
