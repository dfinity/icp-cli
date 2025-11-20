use bigdecimal::BigDecimal;
use candid::{Decode, Encode, Nat, Principal};
use ic_agent::{Agent, AgentError};
use icp_canister_interfaces::{cycles_ledger::CYCLES_LEDGER_CID, icp_ledger::ICP_LEDGER_CID};
use icrc_ledger_types::icrc1::account::Account;
use phf::phf_map;
use snafu::{ResultExt, Snafu};

/// A compile-time map of token names to their corresponding ledger canister IDs.
///
/// This map provides a quick lookup for well-known tokens on the Internet Computer:
/// - "icp": The Internet Computer Protocol token ledger canister
/// - "cycles": The cycles ledger canister for managing computation cycles
///
/// The canister IDs are stored as string literals in textual format.
static TOKEN_LEDGER_CIDS: phf::Map<&'static str, &'static str> = phf_map! {
    "icp" => ICP_LEDGER_CID,
    "cycles" => CYCLES_LEDGER_CID,
};

#[derive(Debug, Snafu)]
pub enum GetBalanceError {
    #[snafu(display("failed to parse canister id '{canister_id}': {source}"))]
    ParseCanisterId {
        canister_id: String,
        source: candid::types::principal::PrincipalError,
    },

    #[snafu(display("failed to get identity principal: {err}"))]
    GetPrincipal { err: String },

    #[snafu(display("failed to query balance"))]
    QueryBalance { source: AgentError },

    #[snafu(display("failed to query decimals"))]
    QueryDecimals { source: AgentError },

    #[snafu(display("failed to query symbol"))]
    QuerySymbol { source: AgentError },

    #[snafu(display("failed to decode balance response"))]
    DecodeBalance { source: candid::Error },

    #[snafu(display("failed to decode decimals response"))]
    DecodeDecimals { source: candid::Error },

    #[snafu(display("failed to decode symbol response"))]
    DecodeSymbol { source: candid::Error },
}

pub struct BalanceInfo {
    pub amount: BigDecimal,
    pub symbol: String,
}

/// Get the token balance for a given identity
///
/// This function queries an ICRC-1 compatible ledger canister to retrieve:
/// - The token balance for the given account
/// - The number of decimals the token uses
/// - The token symbol
///
/// The token parameter supports two flows:
/// 1. Specifying a known token name (e.g., "icp", "cycles") which will be looked up
/// 2. Specifying a canister ID directly for any ICRC-1 compatible ledger
///
/// # Arguments
///
/// * `agent` - The IC agent to use for queries
/// * `token` - The token name or ledger canister id
///
/// # Returns
///
/// A `BalanceInfo` struct containing the formatted amount and token symbol
pub async fn get_balance(
    agent: &Agent,
    token: &str,
) -> Result<BalanceInfo, GetBalanceError> {
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
    let (balance, decimals, symbol) = tokio::join!(
        //
        // Obtain token balance
        async {
            // Convert identity to sender principal
            let owner = agent
                .get_principal()
                .map_err(|err| GetBalanceError::GetPrincipal { err })?;

            // Specify sub-account
            let subaccount = None;

            // Perform query
            let resp = agent
                .query(&cid, "icrc1_balance_of")
                .with_arg(Encode!(&Account { owner, subaccount }).expect("failed to encode arg"))
                .await
                .context(QueryBalanceSnafu)?;

            // Decode response
            Decode!(&resp, Nat).context(DecodeBalanceSnafu)
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
    let (Nat(balance), decimals, symbol) = (
        balance?,         //
        decimals? as i64, //
        symbol?,          //
    );

    // Calculate amount
    let amount = BigDecimal::from_biguint(balance, decimals);

    Ok(BalanceInfo { amount, symbol })
}

