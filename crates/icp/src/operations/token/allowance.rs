use bigdecimal::BigDecimal;
use candid::{Decode, Encode, Nat, Principal};
use ic_agent::{Agent, AgentError};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc2::allowance::{Allowance, AllowanceArgs};
use snafu::{ResultExt, Snafu};

use super::{TOKEN_LEDGER_CIDS, TokenAmount};

#[derive(Debug, Snafu)]
pub enum GetAllowanceError {
    #[snafu(display("failed to parse canister id '{canister_id}': {source}"))]
    ParseCanisterId {
        canister_id: String,
        source: candid::types::principal::PrincipalError,
    },

    #[snafu(display("failed to query decimals"))]
    QueryDecimals { source: AgentError },

    #[snafu(display("failed to query symbol"))]
    QuerySymbol { source: AgentError },

    #[snafu(display("failed to query allowance"))]
    QueryAllowance { source: AgentError },

    #[snafu(display("failed to decode decimals response"))]
    DecodeDecimals { source: candid::Error },

    #[snafu(display("failed to decode symbol response"))]
    DecodeSymbol { source: candid::Error },

    #[snafu(display("failed to decode allowance response"))]
    DecodeAllowance { source: candid::Error },
}

pub struct AllowanceInfo {
    pub allowance: TokenAmount,
    pub expires_at: Option<u64>,
}

/// Get the allowance an owner account has granted to a spender (ICRC-2 `icrc2_allowance`).
///
/// The token parameter supports two flows:
/// 1. Specifying a known token name (e.g., "icp") which will be looked up
/// 2. Specifying a canister ID directly for any ICRC-2 compatible ledger
///
/// # Arguments
///
/// * `agent` - The IC agent to use for queries
/// * `token` - The token name or ledger canister id
/// * `owner` - The principal that granted the allowance
/// * `subaccount` - The owner's subaccount that granted the allowance
/// * `spender` - The account the allowance was granted to
///
/// # Returns
///
/// An `AllowanceInfo` struct containing the allowance amount and optional expiry
pub async fn get_allowance(
    agent: &Agent,
    token: &str,
    owner: Principal,
    subaccount: Option<[u8; 32]>,
    spender: Account,
) -> Result<AllowanceInfo, GetAllowanceError> {
    // Obtain token info
    let canister_id = match TOKEN_LEDGER_CIDS.get(token) {
        // Given token matched known token names
        Some(cid) => cid.to_string(),

        // Given token is not known, indicating it's either already a canister id
        // or is simply a name of a token we do not know of
        None => token.to_string(),
    };

    // Parse the canister id
    let cid = Principal::from_text(&canister_id).context(ParseCanisterIdSnafu {
        canister_id: canister_id.to_string(),
    })?;

    // Perform the required ledger calls
    let (allowance, decimals, symbol) = tokio::join!(
        //
        // Obtain the allowance granted to the spender
        async {
            let arg = AllowanceArgs {
                account: Account { owner, subaccount },
                spender,
            };

            // Perform query
            let resp = agent
                .query(&cid, "icrc2_allowance")
                .with_arg(Encode!(&arg).expect("failed to encode arg"))
                .await
                .context(QueryAllowanceSnafu)?;

            // Decode response
            Decode!(&resp, Allowance).context(DecodeAllowanceSnafu)
        },
        //
        // Obtain the number of decimals the token uses
        async {
            // Perform query
            let resp = agent
                .query(&cid, "icrc1_decimals")
                .with_arg(Encode!(&()).expect("failed to encode arg"))
                .await
                .context(QueryDecimalsSnafu)?;

            // Decode response
            Decode!(&resp, u8).context(DecodeDecimalsSnafu)
        },
        //
        // Obtain the symbol of the token
        async {
            // Perform query
            let resp = agent
                .query(&cid, "icrc1_symbol")
                .with_arg(Encode!(&()).expect("failed to encode arg"))
                .await
                .context(QuerySymbolSnafu)?;

            // Decode response
            Decode!(&resp, String).context(DecodeSymbolSnafu)
        },
    );

    // Check for errors
    let (allowance, decimals, symbol) = (allowance?, decimals? as i64, symbol?);

    let Nat(raw_allowance) = allowance.allowance;

    Ok(AllowanceInfo {
        allowance: TokenAmount {
            amount: BigDecimal::from_biguint(raw_allowance, decimals),
            symbol,
        },
        expires_at: allowance.expires_at,
    })
}
