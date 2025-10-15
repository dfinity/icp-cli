use bigdecimal::{BigDecimal, ToPrimitive};
use candid::{Decode, Encode};
use clap::Parser;
use ic_agent::AgentError;
use ic_ledger_types::{
    AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs, TransferError, TransferResult,
};
use icp::{agent, identity, network};
use icp_canister_interfaces::{
    cycles_ledger::CYCLES_LEDGER_BLOCK_FEE,
    cycles_minting_canister::{
        CYCLES_MINTING_CANISTER_PRINCIPAL, ConversionRateResponse, MEMO_MINT_CYCLES,
        NotifyMintArgs, NotifyMintErr, NotifyMintResponse,
    },
    icp_ledger::{ICP_LEDGER_BLOCK_FEE_E8S, ICP_LEDGER_PRINCIPAL},
};
use tracing::info;

use crate::{
    commands::Context,
    options::{EnvironmentOpt, IdentityOpt},
};

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

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error("Failed to get identity principal: {message}")]
    Principal { message: String },

    #[error("Failed to talk to {canister} canister: {source}")]
    CanisterError {
        canister: String,
        source: AgentError,
    },

    #[error("ICP amount overflow. Specify less tokens.")]
    IcpAmountOverflow,

    #[error("Failed ICP ledger transfer: {src:?}")]
    TransferError { src: TransferError },

    #[error("Insufficient funds: {required} ICP required, {available} ICP available.")]
    InsufficientFunds {
        required: BigDecimal,
        available: BigDecimal,
    },

    #[error("No amount specified. Use --icp-amount or --cycles-amount.")]
    NoAmountSpecified,

    #[error("Failed to notify mint cycles: {src:?}")]
    NotifyMintError { src: NotifyMintErr },
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load project
    let p = ctx.project.load().await?;

    // Load identity
    let id = ctx.identity.load(cmd.identity.into()).await?;

    // Load target environment
    let env =
        p.environments
            .get(cmd.environment.name())
            .ok_or(CommandError::EnvironmentNotFound {
                name: cmd.environment.name().to_owned(),
            })?;

    // Access network
    let access = ctx.network.access(&env.network).await?;

    // Agent
    let agent = ctx.agent.create(id.clone(), &access.url).await?;

    if let Some(k) = access.root_key {
        agent.set_root_key(k);
    }

    // Prepare deposit
    let user_principal = id
        .sender()
        .map_err(|e| CommandError::Principal { message: e })?;

    let icp_e8s_to_deposit = if let Some(icp_amount) = cmd.icp {
        (icp_amount * 100_000_000_u64)
            .to_u64()
            .ok_or(CommandError::IcpAmountOverflow)?
    } else if let Some(cycles_amount) = cmd.cycles {
        let cmc_response = agent
            .query(
                &CYCLES_MINTING_CANISTER_PRINCIPAL,
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
        &CYCLES_MINTING_CANISTER_PRINCIPAL,
        &Subaccount::from(user_principal),
    );
    let memo = Memo(MEMO_MINT_CYCLES);
    let transfer_args = TransferArgs {
        memo,
        amount: Tokens::from_e8s(icp_e8s_to_deposit),
        fee: Tokens::from_e8s(ICP_LEDGER_BLOCK_FEE_E8S),
        from_subaccount: None,
        to: account_id,
        created_at_time: None,
    };

    let transfer_result = agent
        .update(&ICP_LEDGER_PRINCIPAL, "transfer")
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
                    BigDecimal::new((icp_e8s_to_deposit + ICP_LEDGER_BLOCK_FEE_E8S).into(), 8);
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
        .update(&CYCLES_MINTING_CANISTER_PRINCIPAL, "notify_mint_cycles")
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
    info!("Minted {deposited} TCYCLES to your account, new balance: {new_balance} TCYCLES.");

    Ok(())
}
