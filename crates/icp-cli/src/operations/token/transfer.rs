use bigdecimal::{BigDecimal, num_bigint::ToBigInt};
use candid::{Decode, Encode, Nat, Principal};
use ic_agent::{Agent, AgentError};
use icrc_ledger_types::icrc1::{
    account::Account,
    transfer::{TransferArg, TransferError as Icrc1TransferError},
};
use snafu::{ResultExt, Snafu};

use super::TOKEN_LEDGER_CIDS;

#[derive(Debug, Snafu)]
pub enum TokenTransferError {
    #[snafu(display("failed to parse canister id '{canister_id}': {source}"))]
    ParseCanisterId {
        canister_id: String,
        source: candid::types::principal::PrincipalError,
    },

    #[snafu(display("failed to query fee"))]
    QueryFee { source: AgentError },

    #[snafu(display("failed to query decimals"))]
    QueryDecimals { source: AgentError },

    #[snafu(display("failed to query symbol"))]
    QuerySymbol { source: AgentError },

    #[snafu(display("failed to decode fee response"))]
    DecodeFee { source: candid::Error },

    #[snafu(display("failed to decode decimals response"))]
    DecodeDecimals { source: candid::Error },

    #[snafu(display("failed to decode symbol response"))]
    DecodeSymbol { source: candid::Error },

    #[snafu(display("invalid amount: unable to convert to ledger units"))]
    InvalidAmount,

    #[snafu(display("failed to encode transfer argument"))]
    EncodeTransferArg { source: candid::Error },

    #[snafu(display("failed to execute transfer"))]
    ExecuteTransfer { source: AgentError },

    #[snafu(display("failed to decode transfer response"))]
    DecodeTransferResponse { source: candid::Error },

    #[snafu(display(
        "insufficient funds. balance: {balance} {symbol}, required: {required} {symbol}"
    ))]
    InsufficientFunds {
        symbol: String,
        balance: BigDecimal,
        required: BigDecimal,
    },

    #[snafu(display("transfer failed: {message}"))]
    TransferFailed { message: String },
}

pub struct TransferInfo {
    pub block_index: Nat,
    pub amount: BigDecimal,
    pub symbol: String,
    pub receiver: Principal,
}

/// Transfer tokens to a receiver
///
/// This function executes an ICRC-1 token transfer:
/// - Queries the ledger for fee, decimals, and symbol
/// - Converts the decimal amount to ledger units
/// - Executes the transfer
/// - Returns transfer information including the block index
///
/// The token parameter supports two flows:
/// 1. Specifying a known token name (e.g., "icp", "cycles") which will be looked up
/// 2. Specifying a canister ID directly for any ICRC-1 compatible ledger
///
/// # Arguments
///
/// * `agent` - The IC agent to use for queries and updates
/// * `token` - The token name or ledger canister id
/// * `amount` - The decimal amount to transfer
/// * `receiver` - The principal to receive the tokens
///
/// # Returns
///
/// A `TransferInfo` struct containing transfer details including block index
pub async fn transfer(
    agent: &Agent,
    token: &str,
    amount: &BigDecimal,
    receiver: Principal,
) -> Result<TransferInfo, TokenTransferError> {
    // Obtain ledger address
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
    let (fee, decimals, symbol) = tokio::join!(
        //
        // Obtain token transfer fee
        async {
            // Perform query
            let resp = agent
                .query(&cid, "icrc1_fee")
                .with_arg(Encode!(&()).expect("failed to encode arg"))
                .await
                .context(QueryFeeSnafu)?;

            // Decode response
            Decode!(&resp, Nat).context(DecodeFeeSnafu)
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
    let (Nat(fee), decimals, symbol) = (
        fee?,             //
        decimals? as u32, //
        symbol?,          //
    );

    // Calculate units of token to transfer
    // Ledgers do not work in decimals and instead use the smallest non-divisible unit of the token
    let ledger_amount = amount.clone() * 10u128.pow(decimals);

    // Convert amount to big decimal
    let ledger_amount = ledger_amount
        .to_bigint()
        .ok_or(TokenTransferError::InvalidAmount)?
        .to_biguint()
        .ok_or(TokenTransferError::InvalidAmount)?;

    let ledger_amount = Nat::from(ledger_amount);
    let display_amount = BigDecimal::from_biguint(ledger_amount.0.clone(), decimals as i64);

    // Prepare transfer
    let receiver_account = Account {
        owner: receiver,
        subaccount: None,
    };

    let arg = TransferArg {
        // Transfer amount
        amount: ledger_amount.clone(),

        // Transfer destination
        to: receiver_account,

        // Other
        from_subaccount: None,
        fee: None,
        created_at_time: None,
        memo: None,
    };

    // Perform transfer
    let resp = agent
        .update(&cid, "icrc1_transfer")
        .with_arg(Encode!(&arg).context(EncodeTransferArgSnafu)?)
        .call_and_wait()
        .await
        .context(ExecuteTransferSnafu)?;

    // Parse response
    let resp =
        Decode!(&resp, Result<Nat, Icrc1TransferError>).context(DecodeTransferResponseSnafu)?;

    // Process response
    let block_index = resp.map_err(|err| match err {
        // Special case for insufficient funds
        Icrc1TransferError::InsufficientFunds { balance } => {
            let balance = BigDecimal::from_biguint(
                balance.0,       // balance
                decimals as i64, // decimals
            );

            let fee_decimal = BigDecimal::from_biguint(
                fee,             // fee
                decimals as i64, // decimals
            );

            TokenTransferError::InsufficientFunds {
                symbol: symbol.clone(),
                balance,
                required: amount.clone() + fee_decimal,
            }
        }

        _ => TokenTransferError::TransferFailed {
            message: err.to_string(),
        },
    })?;

    Ok(TransferInfo {
        block_index,
        amount: display_amount,
        symbol,
        receiver,
    })
}
