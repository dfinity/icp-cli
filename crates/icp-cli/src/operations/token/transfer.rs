use bigdecimal::{BigDecimal, num_bigint::ToBigInt};
use candid::{Decode, Encode, Nat, Principal};
use ic_agent::{Agent, AgentError};
use icrc_ledger_types::icrc1::{
    account::Account,
    transfer::{TransferArg, TransferError as Icrc1TransferError},
};
use snafu::{ResultExt, Snafu};

use super::{TOKEN_LEDGER_CIDS, TokenAmount};

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

    #[snafu(display("insufficient funds. balance: {balance}, required: {required}"))]
    InsufficientFunds {
        balance: TokenAmount,
        required: TokenAmount,
    },

    #[snafu(display("transfer failed: {message}"))]
    TransferFailed { message: String },
}

pub struct TransferInfo {
    pub block_index: Nat,
    pub transferred: TokenAmount,
    pub receiver: Principal,
}

/// Execute an ICRC-1 transfer with known parameters
///
/// This is a low-level function that performs the actual transfer operation.
/// Use `transfer()` for a higher-level interface that handles token resolution.
///
/// # Arguments
///
/// * `agent` - The IC agent to use for the update call
/// * `ledger_canister_id` - The principal of the ICRC-1 ledger canister
/// * `ledger_amount` - The amount to transfer in ledger units (smallest divisible unit)
/// * `receiver` - The principal to receive the tokens
/// * `fee` - The transfer fee in ledger units
/// * `decimals` - The number of decimals the token uses
/// * `symbol` - The token symbol for display purposes
///
/// # Returns
///
/// A `TransferInfo` struct containing transfer details including block index
pub async fn icrc1_transfer(
    agent: &Agent,
    ledger_canister_id: Principal,
    ledger_amount: Nat,
    receiver: Principal,
    fee: Nat,
    decimals: u32,
    symbol: String,
) -> Result<TransferInfo, TokenTransferError> {
    // Prepare transfer
    let receiver_account = Account {
        owner: receiver,
        subaccount: None,
    };

    let arg = TransferArg {
        amount: ledger_amount.clone(),
        to: receiver_account,
        from_subaccount: None,
        fee: None,
        created_at_time: None,
        memo: None,
    };

    // Perform transfer
    let resp = agent
        .update(&ledger_canister_id, "icrc1_transfer")
        .with_arg(Encode!(&arg).context(EncodeTransferArgSnafu)?)
        .call_and_wait()
        .await
        .context(ExecuteTransferSnafu)?;

    // Parse response
    let resp =
        Decode!(&resp, Result<Nat, Icrc1TransferError>).context(DecodeTransferResponseSnafu)?;

    // Process response
    let block_index = resp.map_err(|err| match err {
        Icrc1TransferError::InsufficientFunds { balance } => {
            let balance_amount = BigDecimal::from_biguint(balance.0, decimals as i64);
            let required_amount =
                BigDecimal::from_biguint(&ledger_amount.0 + fee.0, decimals as i64);

            TokenTransferError::InsufficientFunds {
                balance: TokenAmount {
                    amount: balance_amount,
                    symbol: symbol.clone(),
                },
                required: TokenAmount {
                    amount: required_amount,
                    symbol: symbol.clone(),
                },
            }
        }

        _ => TokenTransferError::TransferFailed {
            message: err.to_string(),
        },
    })?;

    Ok(TransferInfo {
        block_index,
        transferred: TokenAmount {
            amount: BigDecimal::from_biguint(ledger_amount.0, decimals as i64),
            symbol,
        },
        receiver,
    })
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
    let (fee, decimals, symbol) = (fee?, decimals? as u32, symbol?);

    // Calculate units of token to transfer
    // Ledgers do not work in decimals and instead use the smallest non-divisible unit of the token
    let ledger_amount_decimal = amount.clone() * 10u128.pow(decimals);
    let ledger_amount = ledger_amount_decimal
        .to_bigint()
        .ok_or(TokenTransferError::InvalidAmount)?
        .to_biguint()
        .ok_or(TokenTransferError::InvalidAmount)
        .map(Nat::from)?;

    icrc1_transfer(agent, cid, ledger_amount, receiver, fee, decimals, symbol).await
}
