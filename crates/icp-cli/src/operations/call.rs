use candid::{Encode, Nat, Principal};
use ic_agent::Agent;
use icp_canister_interfaces::proxy::{ProxyArgs, ProxyResult};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum UpdateOrProxyCallError {
    #[snafu(display("failed to encode proxy call arguments: {source}"))]
    ProxyEncode { source: candid::Error },

    #[snafu(display("direct update call failed: {source}"))]
    DirectUpdateCall { source: ic_agent::AgentError },

    #[snafu(display("proxy update call failed: {source}"))]
    ProxyUpdateCall { source: ic_agent::AgentError },

    #[snafu(display("failed to decode proxy canister response: {source}"))]
    ProxyDecode { source: candid::Error },

    #[snafu(display("proxy call failed: {message}"))]
    ProxyCall { message: String },
}

/// Dispatches a canister update call, optionally routing through a proxy canister.
///
/// If `proxy` is `None`, makes a direct update call to the target canister.
/// If `proxy` is `Some`, wraps the call in [`ProxyArgs`] and sends it to the
/// proxy canister's `proxy` method, which forwards it to the target.
/// The `cycles` parameter is only used for proxied calls.
pub async fn update_or_proxy_call(
    agent: &Agent,
    canister_id: Principal,
    method: &str,
    arg: Vec<u8>,
    proxy: Option<Principal>,
    cycles: u128,
) -> Result<Vec<u8>, UpdateOrProxyCallError> {
    if let Some(proxy_cid) = proxy {
        let proxy_args = ProxyArgs {
            canister_id,
            method: method.to_string(),
            args: arg,
            cycles: Nat::from(cycles),
        };
        let proxy_arg_bytes = Encode!(&proxy_args).context(ProxyEncodeSnafu)?;

        let proxy_res = agent
            .update(&proxy_cid, "proxy")
            .with_arg(proxy_arg_bytes)
            .await
            .context(ProxyUpdateCallSnafu)?;

        let proxy_result: (ProxyResult,) =
            candid::decode_args(&proxy_res).context(ProxyDecodeSnafu)?;

        match proxy_result.0 {
            ProxyResult::Ok(ok) => Ok(ok.result),
            ProxyResult::Err(err) => ProxyCallSnafu {
                message: err.format_error(),
            }
            .fail(),
        }
    } else {
        let res = agent
            .update(&canister_id, method)
            .with_arg(arg)
            .await
            .context(DirectUpdateCallSnafu)?;
        Ok(res)
    }
}
