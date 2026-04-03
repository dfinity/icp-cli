use candid::utils::{ArgumentDecoder, ArgumentEncoder};
use candid::{Encode, Nat, Principal};
use ic_agent::Agent;
use icp_canister_interfaces::proxy::{ProxyArgs, ProxyResult};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum UpdateOrProxyError {
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

    #[snafu(display("failed to encode call arguments: {source}"))]
    CandidEncode { source: candid::Error },

    #[snafu(display("failed to decode call response: {source}"))]
    CandidDecode { source: candid::Error },
}

/// Dispatches a canister update call, optionally routing through a proxy canister.
///
/// If `proxy` is `None`, makes a direct update call to the target canister.
/// If `proxy` is `Some`, wraps the call in [`ProxyArgs`] and sends it to the
/// proxy canister's `proxy` method, which forwards it to the target.
/// The `cycles` parameter is only used for proxied calls.
pub async fn update_or_proxy_raw(
    agent: &Agent,
    canister_id: Principal,
    method: &str,
    arg: Vec<u8>,
    proxy: Option<Principal>,
    cycles: u128,
) -> Result<Vec<u8>, UpdateOrProxyError> {
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

/// Like [`update_or_proxy_raw`], but accepts typed Candid arguments and decodes the response.
pub async fn update_or_proxy<A, R>(
    agent: &Agent,
    canister_id: Principal,
    method: &str,
    args: A,
    proxy: Option<Principal>,
    cycles: u128,
) -> Result<R, UpdateOrProxyError>
where
    A: ArgumentEncoder,
    R: for<'a> ArgumentDecoder<'a>,
{
    let arg = candid::encode_args(args).context(CandidEncodeSnafu)?;
    let res = update_or_proxy_raw(agent, canister_id, method, arg, proxy, cycles).await?;
    let decoded: R = candid::decode_args(&res).context(CandidDecodeSnafu)?;
    Ok(decoded)
}
