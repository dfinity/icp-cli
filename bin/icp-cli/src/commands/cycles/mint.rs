use bigdecimal::BigDecimal;
use bigdecimal::ToPrimitive;
use candid::{CandidType, Decode, Encode, Nat, Principal};
use clap::Parser;
use ic_agent::AgentError;
use ic_ledger_types::AccountIdentifier;
use ic_ledger_types::Memo;
use ic_ledger_types::Subaccount;
use ic_ledger_types::Tokens;
use ic_ledger_types::TransferArgs;
use ic_ledger_types::TransferResult;
use icp_identity::key::LoadIdentityInContextError;
use serde::Deserialize;
use snafu::Snafu;

use crate::{
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
};

pub const MEMO_MINT_CYCLES: u64 = 0x544e494d; // == 'MINT'

#[derive(Debug, Parser)]
pub struct Cmd {
    #[clap(flatten)]
    pub environment: EnvironmentOpt,

    #[clap(flatten)]
    pub identity: IdentityOpt,

    #[clap(long = "icp-amount", conflicts_with = "cycles_amount")]
    pub icp_amount: Option<BigDecimal>,

    #[clap(long = "cycles-amount", conflicts_with = "icp_amount")]
    pub cycles_amount: Option<u128>,
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

    let icp_e8s_to_deposit = if let Some(icp_amount) = cmd.icp_amount {
        (icp_amount * 100_000_000_u64)
            .to_u64()
            .ok_or(CommandError::IcpAmountOverflow)?
    } else if let Some(cycles_amount) = cmd.cycles_amount {
        let cmc_response = agent
            .query(
                &Principal::from_text("rkp4c-7iaaa-aaaaa-aaaca-cai").unwrap(),
                "get_icp_xdr_conversion_rate",
            )
            .with_arg(Encode!(&()).unwrap())
            .call()
            .await
            .map_err(|e| CommandError::CanisterError {
                canister: "cmc".to_string(),
                source: e,
            })?;

        let cmc_response = Decode!(&cmc_response, CmcResponse).unwrap();
        let cycles_per_e8s = cmc_response.data.xdr_permyriad_per_icp as u128;
        let cycles_plus_fees = cycles_amount + 100_000_000_u128; // Cycles ledger charges 100M for deposits
        let e8s_to_deposit = cycles_plus_fees.div_ceil(cycles_per_e8s);

        e8s_to_deposit
            .to_u64()
            .ok_or(CommandError::IcpAmountOverflow)?
    } else {
        return Err(CommandError::NoAmountSpecified);
    };

    let account_id = AccountIdentifier::new(
        &Principal::from_text("rkp4c-7iaaa-aaaaa-aaaca-cai").unwrap(),
        &Subaccount::from(user_principal),
    );
    let memo = Memo(MEMO_MINT_CYCLES);
    let transfer_args = TransferArgs {
        memo,
        amount: Tokens::from_e8s(icp_e8s_to_deposit),
        fee: Tokens::from_e8s(10_000),
        from_subaccount: None,
        to: account_id,
        created_at_time: None,
    };

    let transfer_result = agent
        .update(
            &Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
            "transfer",
        )
        .with_arg(Encode!(&transfer_args).unwrap())
        .call_and_wait()
        .await
        .map_err(|e| CommandError::CanisterError {
            canister: "ICP ledger".to_string(),
            source: e,
        })?;
    let transfer_response = Decode!(&transfer_result, TransferResult).unwrap();
    let block_index = match transfer_response {
        Ok(block_index) => block_index,
        Err(err) => {
            match err {
                ic_ledger_types::TransferError::TxDuplicate { duplicate_of } => duplicate_of,
                ic_ledger_types::TransferError::InsufficientFunds { balance } => {
                    let required = BigDecimal::new((icp_e8s_to_deposit + 10_000).into(), 8); // transfer fee
                    let available = BigDecimal::new(balance.e8s().into(), 8);
                    return Err(CommandError::InsufficientFunds {
                        required,
                        available,
                    });
                }
                err => {
                    return Err(CommandError::TransferError { src: err });
                }
            }
        }
    };

    let notify_response = agent
        .update(
            &Principal::from_text("rkp4c-7iaaa-aaaaa-aaaca-cai").unwrap(),
            "notify_mint_cycles",
        )
        .with_arg(
            Encode!(&NotifyMintCyclesArgs {
                block_index,
                deposit_memo: None,
                to_subaccount: None,
            })
            .unwrap(),
        )
        .call_and_wait()
        .await
        .map_err(|e| CommandError::CanisterError {
            canister: "cmc".to_string(),
            source: e,
        })?;
    let notify_response = Decode!(&notify_response, NotifyMintCyclesResponse).unwrap();
    let minted = match notify_response {
        NotifyMintCyclesResponse::Ok(ok) => ok,
        NotifyMintCyclesResponse::Err(err) => {
            return Err(CommandError::NotifyMintCyclesError { src: err });
        }
    };

    // display
    let deposited = BigDecimal::new((minted.minted - 100_000_000_u128).into(), 12); // deposit charges 100M fee
    let new_balance = BigDecimal::new(minted.balance.into(), 12);
    println!("Minted {deposited} TCYCLES to your account, new balance: {new_balance} TCYCLES.");

    Ok(())
}

#[derive(Debug, Deserialize, CandidType)]
struct CmcResponse {
    data: CmcData,
}

#[derive(Debug, Deserialize, CandidType)]
struct CmcData {
    xdr_permyriad_per_icp: u64,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintCyclesArgs {
    pub block_index: u64,
    pub deposit_memo: Option<Vec<u8>>,
    pub to_subaccount: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintCyclesOk {
    pub balance: Nat,
    pub block_index: Nat,
    pub minted: Nat,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintCyclesRefunded {
    pub block_index: Option<u64>,
    pub reason: String,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyMintCyclesOther {
    pub error_message: String,
    pub error_code: u64,
}

#[derive(Debug, Deserialize, CandidType)]
pub enum NotifyMintCyclesErr {
    Refunded(NotifyMintCyclesRefunded),
    InvalidTransaction(String),
    Other(NotifyMintCyclesOther),
    Processing,
    TransactionTooOld(u64),
}

#[derive(Debug, Deserialize, CandidType)]
pub enum NotifyMintCyclesResponse {
    Ok(NotifyMintCyclesOk),
    Err(NotifyMintCyclesErr),
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
    GetAgent { source: ContextGetAgentError },

    #[snafu(display("Failed to get identity principal: {message}"))]
    GetPrincipalError { message: String },

    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("ICP amount overflow. Specify less tokens."))]
    IcpAmountOverflow,

    #[snafu(display("Failed ICP ledger transfer: {src:?}"))]
    TransferError { src: ic_ledger_types::TransferError },

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
    NotifyMintCyclesError { src: NotifyMintCyclesErr },
}
