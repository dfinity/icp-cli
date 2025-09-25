use bigdecimal::{BigDecimal, ToPrimitive};
use candid::{CandidType, Decode, Encode, Nat, Principal};
use clap::Parser;
use ic_agent::AgentError;
use ic_ledger_types::{
    AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs, TransferError, TransferResult,
};
use icp_identity::key::LoadIdentityInContextError;
use serde::Deserialize;
use snafu::Snafu;

use crate::{
    CYCLES_MINTING_CANISTER_CID, ICP_LEDGER_CID,
    context::{Context, ContextAgentError, ContextProjectError},
    options::{EnvironmentOpt, IdentityOpt},
};

pub const MEMO_MINT_CYCLES: u64 = 0x544e494d; // == 'MINT'
/// 0.0001 ICP, a.k.a. 10k e8s
const ICP_TRANSFER_FEE_E8S: u64 = 10_000;
/// 100m cycles
const CYCLES_LEDGER_BLOCK_FEE: u128 = 100_000_000;

#[derive(Debug, Parser)]
pub struct Cmd {
    /// Amount of ICP to mint to cycles.
    #[arg(long, conflicts_with = "cycles")]
    pub icp: Option<BigDecimal>,

    /// Amount of cycles to mint. Automatically determines the amount of ICP needed.
    #[arg(long, conflicts_with = "icp")]
    pub cycles: Option<u128>,

    #[command(flatten)]
    pub environment: EnvironmentOpt,

    #[command(flatten)]
    pub identity: IdentityOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load identity
    ctx.require_identity(cmd.identity.name());

    // Load the project manifest
    let pm = ctx.project()?;

    // Load target environment
    let env = pm
        .environments
        .iter()
        .find(|&v| v.name == cmd.environment.name())
        .ok_or(CommandError::EnvironmentNotFound {
            name: cmd.environment.name().to_owned(),
        })?;

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

    // Setup network
    ctx.require_network(network);

    // Prepare agent
    let agent = ctx.agent()?;
    let user_principal = ctx
        .identity()?
        .sender()
        .map_err(|e| CommandError::GetPrincipalError { message: e })?;

    let icp_e8s_to_deposit = if let Some(icp_amount) = cmd.icp {
        (icp_amount * 100_000_000_u64)
            .to_u64()
            .ok_or(CommandError::IcpAmountOverflow)?
    } else if let Some(cycles_amount) = cmd.cycles {
        let cmc_response = agent
            .query(
                &Principal::from_text(CYCLES_MINTING_CANISTER_CID).unwrap(),
                "get_icp_xdr_conversion_rate",
            )
            .with_arg(Encode!(&()).expect("Failed to encode get ICP XDR conversion rate args"))
            .call()
            .await
            .map_err(|e| CommandError::CanisterError {
                canister: "cmc".to_string(),
                source: e,
            })?;

        let cmc_response =
            Decode!(&cmc_response, ConversionRateResponse).expect("CMC response type changed");
        let cycles_per_e8s = cmc_response.data.xdr_permyriad_per_icp as u128;
        let cycles_plus_fees = cycles_amount + CYCLES_LEDGER_BLOCK_FEE;
        let e8s_to_deposit = cycles_plus_fees.div_ceil(cycles_per_e8s);

        e8s_to_deposit
            .to_u64()
            .ok_or(CommandError::IcpAmountOverflow)?
    } else {
        return Err(CommandError::NoAmountSpecified);
    };

    let account_id = AccountIdentifier::new(
        &Principal::from_text(CYCLES_MINTING_CANISTER_CID).unwrap(),
        &Subaccount::from(user_principal),
    );
    let memo = Memo(MEMO_MINT_CYCLES);
    let transfer_args = TransferArgs {
        memo,
        amount: Tokens::from_e8s(icp_e8s_to_deposit),
        fee: Tokens::from_e8s(ICP_TRANSFER_FEE_E8S),
        from_subaccount: None,
        to: account_id,
        created_at_time: None,
    };

    let transfer_result = agent
        .update(&Principal::from_text(ICP_LEDGER_CID).unwrap(), "transfer")
        .with_arg(Encode!(&transfer_args).expect("Failed to encode transfer args"))
        .call_and_wait()
        .await
        .map_err(|e| CommandError::CanisterError {
            canister: "ICP ledger".to_string(),
            source: e,
        })?;
    let transfer_response =
        Decode!(&transfer_result, TransferResult).expect("ICP ledger transfer result type changed");
    let block_index = match transfer_response {
        Ok(block_index) => block_index,
        Err(err) => match err {
            TransferError::TxDuplicate { duplicate_of } => duplicate_of,
            TransferError::InsufficientFunds { balance } => {
                let required =
                    BigDecimal::new((icp_e8s_to_deposit + ICP_TRANSFER_FEE_E8S).into(), 8);
                let available = BigDecimal::new(balance.e8s().into(), 8);
                return Err(CommandError::InsufficientFunds {
                    required,
                    available,
                });
            }
            err => {
                return Err(CommandError::TransferError { src: err });
            }
        },
    };

    let notify_response = agent
        .update(
            &Principal::from_text(CYCLES_MINTING_CANISTER_CID).unwrap(),
            "notify_mint_cycles",
        )
        .with_arg(
            Encode!(&NotifyMintArgs {
                block_index,
                deposit_memo: None,
                to_subaccount: None,
            })
            .expect("Failed to encode notify mint cycles args"),
        )
        .call_and_wait()
        .await
        .map_err(|e| CommandError::CanisterError {
            canister: "cmc".to_string(),
            source: e,
        })?;
    let notify_response = Decode!(&notify_response, NotifyMintResponse)
        .expect("Notify mint cycles response type changed");
    let minted = match notify_response {
        NotifyMintResponse::Ok(ok) => ok,
        NotifyMintResponse::Err(err) => {
            return Err(CommandError::NotifyMintError { src: err });
        }
    };

    // display
    let deposited = BigDecimal::new((minted.minted - CYCLES_LEDGER_BLOCK_FEE).into(), 12);
    let new_balance = BigDecimal::new(minted.balance.into(), 12);
    let _ = ctx.term.write_line(&format!(
        "Minted {deposited} TCYCLES to your account, new balance: {new_balance} TCYCLES."
    ));

    Ok(())
}

/// Response from get_icp_xdr_conversion_rate on the cycles minting canister
#[derive(Debug, Deserialize, CandidType)]
struct ConversionRateResponse {
    data: ConversionRateData,
}

#[derive(Debug, Deserialize, CandidType)]
struct ConversionRateData {
    xdr_permyriad_per_icp: u64,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintArgs {
    pub block_index: u64,
    pub deposit_memo: Option<Vec<u8>>,
    pub to_subaccount: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintOk {
    pub balance: Nat,
    pub block_index: Nat,
    pub minted: Nat,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintRefunded {
    pub block_index: Option<u64>,
    pub reason: String,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintOther {
    pub error_message: String,
    pub error_code: u64,
}

#[derive(Debug, Deserialize, CandidType)]
pub enum NotifyMintErr {
    Refunded(NotifyMintRefunded),
    InvalidTransaction(String),
    Other(NotifyMintOther),
    Processing,
    TransactionTooOld(u64),
}

#[derive(Debug, Deserialize, CandidType)]
pub enum NotifyMintResponse {
    Ok(NotifyMintOk),
    Err(NotifyMintErr),
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(display("Failed to talk to {canister} canister: {source}"))]
    CanisterError {
        canister: String,
        source: AgentError,
    },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    GetAgent { source: ContextAgentError },

    #[snafu(display("Failed to get identity principal: {message}"))]
    GetPrincipalError { message: String },

    #[snafu(transparent)]
    GetProject { source: ContextProjectError },

    #[snafu(display("ICP amount overflow. Specify less tokens."))]
    IcpAmountOverflow,

    #[snafu(display("Failed ICP ledger transfer: {src:?}"))]
    TransferError { src: TransferError },

    #[snafu(display("Insufficient funds: {required} ICP required, {available} ICP available."))]
    InsufficientFunds {
        required: BigDecimal,
        available: BigDecimal,
    },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("No amount specified. Use --icp-amount or --cycles-amount."))]
    NoAmountSpecified,

    #[snafu(display("Failed to notify mint cycles: {src:?}"))]
    NotifyMintError { src: NotifyMintErr },
}
