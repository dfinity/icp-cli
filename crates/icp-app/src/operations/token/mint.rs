use bigdecimal::{BigDecimal, ToPrimitive};
use candid::{Decode, Encode};
use ic_agent::{Agent, AgentError};
use ic_ledger_types::{
    AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs, TransferError, TransferResult,
};
use icp_canister_interfaces::{
    cycles_ledger::CYCLES_LEDGER_BLOCK_FEE,
    cycles_minting_canister::{
        CYCLES_MINTING_CANISTER_PRINCIPAL, ConversionRateResponse, MEMO_MINT_CYCLES,
        NotifyMintArgs, NotifyMintResponse,
    },
    icp_ledger::{ICP_LEDGER_BLOCK_FEE_E8S, ICP_LEDGER_PRINCIPAL, ICP_LEDGER_SYMBOL},
};
use snafu::{ResultExt, Snafu};

use super::TokenAmount;

#[derive(Debug, Snafu)]
pub enum MintCyclesError {
    #[snafu(display("Failed to get identity principal: {message}"))]
    GetPrincipal { message: String },

    #[snafu(display("Failed to query CMC for conversion rate"))]
    QueryConversionRate { source: AgentError },

    #[snafu(display("Failed to transfer ICP to CMC"))]
    TransferIcp { source: AgentError },

    #[snafu(display("Failed to notify CMC of mint"))]
    NotifyMint { source: AgentError },

    #[snafu(display("ICP amount overflow. Specify less tokens."))]
    IcpAmountOverflow,

    #[snafu(display("Failed ICP ledger transfer: {message}"))]
    TransferFailed { message: String },

    #[snafu(display("Insufficient funds: {required} required, {available} available."))]
    InsufficientFunds {
        required: TokenAmount,
        available: TokenAmount,
    },

    #[snafu(display("No amount specified. Must provide either ICP or cycles amount."))]
    NoAmountSpecified,

    #[snafu(display("Failed to notify mint cycles: {message}"))]
    NotifyMintFailed { message: String },
}

pub struct MintInfo {
    pub deposited: TokenAmount,
    pub new_balance: TokenAmount,
}

/// Mint cycles from ICP
///
/// This function executes the full cycle minting flow:
/// 1. Determines how much ICP to deposit (either directly or by calculating from desired cycles)
/// 2. Transfers ICP to the Cycles Minting Canister (CMC)
/// 3. Notifies the CMC to mint cycles
/// 4. Returns information about the minted cycles
///
/// # Arguments
///
/// * `agent` - The IC agent to use for queries and updates
/// * `icp_amount` - Optional ICP amount to convert to cycles
/// * `cycles_amount` - Optional desired cycles amount (will calculate required ICP)
///
/// One of `icp_amount` or `cycles_amount` must be provided (but not both).
///
/// # Returns
///
/// A `MintInfo` struct containing the deposited amount (minus fees) and new balance
pub async fn mint_cycles(
    agent: &Agent,
    icp_amount: Option<&BigDecimal>,
    cycles_amount: Option<u128>,
    from_subaccount: Option<[u8; 32]>,
    to_subaccount: Option<[u8; 32]>,
) -> Result<MintInfo, MintCyclesError> {
    // Get user principal
    let user_principal = agent
        .get_principal()
        .map_err(|e| MintCyclesError::GetPrincipal { message: e })?;

    // Calculate ICP e8s to deposit
    let icp_e8s_to_deposit = if let Some(icp_amount) = icp_amount {
        (icp_amount * 100_000_000_u64)
            .to_u64()
            .ok_or(MintCyclesError::IcpAmountOverflow)?
    } else if let Some(cycles_amount) = cycles_amount {
        // Query CMC for conversion rate
        let cmc_response = agent
            .query(
                &CYCLES_MINTING_CANISTER_PRINCIPAL,
                "get_icp_xdr_conversion_rate",
            )
            .with_arg(Encode!(&()).expect("Failed to encode get ICP XDR conversion rate args"))
            .call()
            .await
            .context(QueryConversionRateSnafu)?;

        let cmc_response =
            Decode!(&cmc_response, ConversionRateResponse).expect("CMC response type changed");
        let cycles_per_e8s = cmc_response.data.xdr_permyriad_per_icp as u128;
        let cycles_plus_fees = cycles_amount + CYCLES_LEDGER_BLOCK_FEE;
        let e8s_to_deposit = cycles_plus_fees.div_ceil(cycles_per_e8s);

        e8s_to_deposit
            .to_u64()
            .ok_or(MintCyclesError::IcpAmountOverflow)?
    } else {
        return NoAmountSpecifiedSnafu.fail();
    };

    // Prepare transfer to CMC
    let account_id = AccountIdentifier::new(
        &CYCLES_MINTING_CANISTER_PRINCIPAL,
        &Subaccount::from(user_principal),
    );
    let memo = Memo(MEMO_MINT_CYCLES);
    let transfer_args = TransferArgs {
        memo,
        amount: Tokens::from_e8s(icp_e8s_to_deposit),
        fee: Tokens::from_e8s(ICP_LEDGER_BLOCK_FEE_E8S),
        from_subaccount: from_subaccount.map(Subaccount),
        to: account_id,
        created_at_time: None,
    };

    // Execute ICP transfer
    let transfer_result = agent
        .update(&ICP_LEDGER_PRINCIPAL, "transfer")
        .with_arg(Encode!(&transfer_args).expect("Failed to encode transfer args"))
        .call_and_wait()
        .await
        .context(TransferIcpSnafu)?;

    let transfer_response =
        Decode!(&transfer_result, TransferResult).expect("ICP ledger transfer result type changed");

    // Handle transfer result
    let block_index = match transfer_response {
        Ok(block_index) => block_index,
        Err(err) => match err {
            TransferError::TxDuplicate { duplicate_of } => duplicate_of,
            TransferError::InsufficientFunds { balance } => {
                let required_amount =
                    BigDecimal::new((icp_e8s_to_deposit + ICP_LEDGER_BLOCK_FEE_E8S).into(), 8);
                let available_amount = BigDecimal::new(balance.e8s().into(), 8);
                return InsufficientFundsSnafu {
                    required: TokenAmount {
                        amount: required_amount,
                        symbol: ICP_LEDGER_SYMBOL.to_string(),
                    },
                    available: TokenAmount {
                        amount: available_amount,
                        symbol: ICP_LEDGER_SYMBOL.to_string(),
                    },
                }
                .fail();
            }
            err => {
                return TransferFailedSnafu {
                    message: format!("{:?}", err),
                }
                .fail();
            }
        },
    };

    // Notify CMC to mint cycles
    let notify_response = agent
        .update(&CYCLES_MINTING_CANISTER_PRINCIPAL, "notify_mint_cycles")
        .with_arg(
            Encode!(&NotifyMintArgs {
                block_index,
                deposit_memo: None,
                to_subaccount: to_subaccount.map(Vec::from),
            })
            .expect("Failed to encode notify mint cycles args"),
        )
        .call_and_wait()
        .await
        .context(NotifyMintSnafu)?;

    let notify_response = Decode!(&notify_response, NotifyMintResponse)
        .expect("Notify mint cycles response type changed");

    // Handle notify result
    let minted = match notify_response {
        NotifyMintResponse::Ok(ok) => ok,
        NotifyMintResponse::Err(err) => {
            return Err(MintCyclesError::NotifyMintFailed {
                message: format!("{:?}", err),
            });
        }
    };

    let deposited_amount = BigDecimal::new((minted.minted - CYCLES_LEDGER_BLOCK_FEE).into(), 0);
    let new_balance_amount = BigDecimal::new(minted.balance.into(), 0);

    Ok(MintInfo {
        deposited: TokenAmount {
            amount: deposited_amount,
            symbol: "cycles".to_string(),
        },
        new_balance: TokenAmount {
            amount: new_balance_amount,
            symbol: "cycles".to_string(),
        },
    })
}
