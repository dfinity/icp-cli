use bigdecimal::{BigDecimal, num_bigint::ToBigInt};
use candid::{Decode, Encode, Nat, Principal};
use ic_agent::{Agent, AgentError};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc2::approve::{ApproveArgs, ApproveError as Icrc2ApproveError};
use snafu::{ResultExt, Snafu};

use super::{TOKEN_LEDGER_CIDS, TokenAmount};

#[derive(Debug, Snafu)]
pub enum TokenApproveError {
    #[snafu(display("failed to parse canister id '{canister_id}'"))]
    ParseCanisterId {
        canister_id: String,
        source: candid::types::principal::PrincipalError,
    },

    #[snafu(display("failed to query decimals"))]
    QueryDecimals { source: AgentError },

    #[snafu(display("failed to query symbol"))]
    QuerySymbol { source: AgentError },

    #[snafu(display("failed to decode decimals response"))]
    DecodeDecimals { source: candid::Error },

    #[snafu(display("failed to decode symbol response"))]
    DecodeSymbol { source: candid::Error },

    #[snafu(display("invalid amount: unable to convert to ledger units"))]
    InvalidAmount,

    #[snafu(display("failed to encode approve argument"))]
    EncodeApproveArg { source: candid::Error },

    #[snafu(display("failed to execute approve"))]
    ExecuteApprove { source: AgentError },

    #[snafu(display("failed to decode approve response"))]
    DecodeApproveResponse { source: candid::Error },

    #[snafu(display("approve failed: {message}"))]
    ApproveFailed { message: String },
}

pub struct ApproveInfo {
    pub block_index: Nat,
    pub allowance: TokenAmount,
    pub spender_display: String,
}

/// Approve a spender to transfer tokens on the caller's behalf (ICRC-2 `icrc2_approve`).
///
/// This sets the spender's allowance to `amount`, overwriting any existing allowance.
/// The approval fee is charged to the caller's account (optionally scoped to
/// `from_subaccount`); the ledger's default fee is used.
///
/// The token parameter supports two flows:
/// 1. Specifying a known token name (e.g., "icp") which will be looked up
/// 2. Specifying a canister ID directly for any ICRC-2 compatible ledger
///
/// # Arguments
///
/// * `agent` - The IC agent to use for queries and the update call
/// * `token` - The token name or ledger canister id
/// * `amount` - The decimal allowance amount to grant
/// * `from_subaccount` - The caller's subaccount to grant the allowance from
/// * `spender` - The account being granted the allowance
/// * `expires_at` - Optional absolute expiry, in nanoseconds since the Unix epoch
///
/// # Returns
///
/// An `ApproveInfo` struct containing the block index and the granted allowance
pub async fn approve(
    agent: &Agent,
    token: &str,
    amount: &BigDecimal,
    from_subaccount: Option<[u8; 32]>,
    spender: Account,
    expires_at: Option<u64>,
) -> Result<ApproveInfo, TokenApproveError> {
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
    let (decimals, symbol) = tokio::join!(
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
    let (decimals, symbol) = (decimals? as u32, symbol?);

    // Calculate units of token to approve
    // Ledgers do not work in decimals and instead use the smallest non-divisible unit of the token
    let ledger_amount_decimal = amount.clone() * 10u128.pow(decimals);
    let ledger_amount = ledger_amount_decimal
        .to_bigint()
        .ok_or(TokenApproveError::InvalidAmount)?
        .to_biguint()
        .ok_or(TokenApproveError::InvalidAmount)
        .map(Nat::from)?;

    // Capture the spender display before the value is moved into the argument
    let spender_display = spender.to_string();

    let arg = ApproveArgs {
        from_subaccount,
        spender,
        amount: ledger_amount.clone(),
        expected_allowance: None,
        expires_at,
        fee: None,
        memo: None,
        created_at_time: None,
    };

    // Perform approve
    let resp = agent
        .update(&cid, "icrc2_approve")
        .with_arg(Encode!(&arg).context(EncodeApproveArgSnafu)?)
        .call_and_wait()
        .await
        .context(ExecuteApproveSnafu)?;

    // Parse response
    let resp =
        Decode!(&resp, Result<Nat, Icrc2ApproveError>).context(DecodeApproveResponseSnafu)?;

    // Process response
    let block_index = resp.map_err(|err| TokenApproveError::ApproveFailed {
        message: err.to_string(),
    })?;

    Ok(ApproveInfo {
        block_index,
        allowance: TokenAmount {
            amount: BigDecimal::from_biguint(ledger_amount.0, decimals as i64),
            symbol,
        },
        spender_display,
    })
}
