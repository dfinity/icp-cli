use base64::engine::{Engine as _, general_purpose::URL_SAFE_NO_PAD};
use candid::{Decode, Encode};
use ic_agent::{Agent, export::Principal};
use icp::{identity::delegation::DelegationChain, network::custom_domains, signal};
use indicatif::{ProgressBar, ProgressStyle};
use snafu::{ResultExt, Snafu};
use url::Url;

#[derive(Debug, Snafu)]
pub(crate) enum IiPollError {
    #[snafu(display("failed to open browser"))]
    OpenBrowser { source: std::io::Error },

    #[snafu(display("failed to query cli-backend canister"))]
    Query { source: ic_agent::AgentError },

    #[snafu(display("failed to decode candid response"))]
    CandidDecode { source: candid::Error },

    #[snafu(display("interrupted"))]
    Interrupted,
}

/// Opens a browser for II authentication and polls the cli-backend canister
/// until the delegation chain is stored. Returns the received delegation chain.
pub(crate) async fn poll_for_delegation(
    agent: &Agent,
    clii_backend_id: Principal,
    clii_frontend_id: Principal,
    der_public_key: &[u8],
    http_gateway_url: &Url,
    friendly_name: Option<(&str, &str)>,
) -> Result<DelegationChain, IiPollError> {
    let uuid = uuid::Uuid::new_v4().to_string();
    let key_b64 = URL_SAFE_NO_PAD.encode(der_public_key);

    let mut frontend_url =
        custom_domains::canister_gateway_url(http_gateway_url, clii_frontend_id, friendly_name);
    frontend_url.set_query(Some(&format!("k={key_b64}&uuid={uuid}")));

    tracing::info!("Opening browser for Internet Identity authentication...");
    tracing::debug!("Frontend URL: {frontend_url}");
    open::that(frontend_url.as_str()).context(OpenBrowserSnafu)?;

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("valid template"),
    );
    spinner.set_message("Waiting for Internet Identity authentication...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    let args = Encode!(&uuid).expect("infallible candid encode");

    loop {
        tokio::select! {
            _ = signal::stop_signal() => {
                spinner.finish_and_clear();
                return InterruptedSnafu.fail();
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {
                let response = agent
                    .query(&clii_backend_id, "get_delegation")
                    .with_arg(args.clone())
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
