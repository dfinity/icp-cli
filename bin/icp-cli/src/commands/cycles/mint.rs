use bigdecimal::{BigDecimal, ToPrimitive};
use candid::{CandidType, Decode, Encode, Nat, Principal};
use clap::{Args, Parser};
use ic_agent::AgentError;
use ic_ledger_types::{AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs, TransferError};
use icp_identity::key::LoadIdentityInContextError;
use serde::Deserialize;
use snafu::Snafu;

use crate::{
    CYCLES_MINTER_CID, ICP_LEDGER_CID,
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
};

/// Memo value for mint operations.
/// This constant represents the ASCII encoding of the string "MINT" as a 64-bit unsigned integer.
const MEMO_MINT: u64 = 0x544e494d;

/// Number of e8s (10^-8 units) in 1 ICP token.
/// This constant is used for converting between ICP amounts and their smallest unit representation.
const ICP_E8S: u64 = 100_000_000;

/// fee (in cycles) for depositing to cycles ledger
///
const CYCLES_LEDGER_DEPOSIT_FEE: u128 = 100_000_000;

/// hard-coded fee (in icp) to make transfer
///
const ICP_TRANSFER_FEE: u64 = 10_000;

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct Amount {
    /// ICP amount to convert to cycles (conflicts with cycles option)
    #[clap(long)]
    pub icp: Option<BigDecimal>,

    /// Cycles amount to mint (conflicts with icp option)
    #[clap(long)]
    pub cycles: Option<u128>,
}

#[derive(Debug, Parser)]
pub struct Cmd {
    #[command(flatten)]
    amount: Amount,

    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,
}

#[derive(Debug, Deserialize, CandidType)]
struct RateData {
    xdr_permyriad_per_icp: u64,
}

#[derive(Debug, Deserialize, CandidType)]
struct RateResponse {
    data: RateData,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyArgs {
    pub block_index: u64,
    pub deposit_memo: Option<Vec<u8>>,
    pub to_subaccount: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyErrorRefunded {
    pub block_index: Option<u64>,
    pub reason: String,
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyErrorOther {
    pub error_message: String,
    pub error_code: u64,
}

#[derive(Debug, Deserialize, CandidType)]
pub enum NotifyError {
    InvalidTransaction(String),
    Other(NotifyErrorOther),
    Processing,
    Refunded(NotifyErrorRefunded),
    TransactionTooOld(u64),
}

#[derive(Debug, Deserialize, CandidType)]
pub struct NotifyOk {
    pub balance: Nat,
    pub block_index: Nat,
    pub minted: Nat,
}

#[derive(Debug, Deserialize, CandidType)]
pub enum NotifyResponse {
    Ok(NotifyOk),
    Err(NotifyError),
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest
    let pm = ctx.project()?;

    // Load identity
    ctx.require_identity(cmd.identity.name());

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

    // Parse the canister id
    let cid =
        Principal::from_text(CYCLES_MINTER_CID).map_err(|err| CommandError::GetPrincipal {
            err: err.to_string(),
        })?;

    // Convert from icp/cycles to icp e8s amount
    let icp = match (cmd.amount.icp, cmd.amount.cycles) {
        // icp amount specified
        (Some(icp), None) => (icp * ICP_E8S)
            .to_u64()
            .ok_or(CommandError::IcpAmountOverflow)?,

        // cycles amount specified
        (None, Some(cycles)) => {
            // Perform query to cycles minting canister to get current conversion rate
            let resp = agent
                .query(&cid, "get_icp_xdr_conversion_rate")
                .with_arg(Encode!(&()).expect("failed to encode arg"))
                .await?;

            // Decode response
            let resp = Decode!(&resp, RateResponse)?;

            // Current cycles price in icp
            let cost = resp.data.xdr_permyriad_per_icp as u128;

            // Convert to icp
            (cycles + CYCLES_LEDGER_DEPOSIT_FEE)
                .div_ceil(cost)
                .to_u64()
                .ok_or(CommandError::IcpAmountOverflow)?
        }

        // invalid
        _ => unreachable!(),
    };

    // Load identity
    let id = ctx.identity()?;

    // Convert identity to sender principal
    let sender = id
        .sender()
        .map_err(|err| CommandError::GetPrincipal { err })?;

    // Prepare deposit
    let receiver = AccountIdentifier::new(
        &cid,                      // owner
        &Subaccount::from(sender), // subaccount
    );

    let arg = TransferArgs {
        memo: Memo(MEMO_MINT),

        // Transfer amount and fee
        amount: Tokens::from_e8s(icp),
        fee: Tokens::from_e8s(ICP_TRANSFER_FEE),

        // Transfer target
        to: receiver,
        from_subaccount: None,

        // Other
        created_at_time: None,
    };

    // Perform transfer
    let cid = Principal::from_text(ICP_LEDGER_CID).map_err(|err| CommandError::GetPrincipal {
        err: err.to_string(),
    })?;

    let resp = agent
        .update(&cid, "transfer")
        .with_arg(Encode!(&arg)?)
        .call_and_wait()
        .await?;

    // Parse response
    let mut resp = Decode!(&resp, Result<u64, TransferError>)?;

    // If it's a duplicate, simply convert the error to a successful result
    if let Err(TransferError::TxDuplicate { duplicate_of: idx }) = resp {
        resp = Ok(idx);
    }

    // Process response
    let idx = resp.map_err(|err| match err {
        // Special case for insufficient funds
        TransferError::InsufficientFunds { balance } => {
            let balance = BigDecimal::new(
                balance.e8s().into(), // digits
                8,                    // scale
            );

            let fee = BigDecimal::new(
                ICP_TRANSFER_FEE.into(), // digits
                8,                       // scale
            );

            CommandError::InsufficientFunds {
                symbol: "ICP".into(),
                balance,
                required: icp + fee,
            }
        }

        _ => CommandError::Transfer {
            err: err.to_string(),
        },
    })?;

    // Notify the cycles minter the deposit was made
    let cid =
        Principal::from_text(CYCLES_MINTER_CID).map_err(|err| CommandError::GetPrincipal {
            err: err.to_string(),
        })?;

    let arg = NotifyArgs {
        block_index: idx,
        deposit_memo: None,
        to_subaccount: None,
    };

    let resp = agent
        .update(&cid, "notify_mint_cycles")
        .with_arg(Encode!(&arg)?)
        .call_and_wait()
        .await?;

    // Parse response
    let resp = Decode!(&resp, NotifyResponse)?;

    // Convert response
    let resp = match resp {
        // Success
        NotifyResponse::Ok(v) => Ok(v),

        // Failure
        NotifyResponse::Err(err) => Err(CommandError::Transfer {
            err: format!("{err:?}"),
        }),
    }?;

    let NotifyOk {
        balance, minted, ..
    } = resp;

    // Convert units
    let amount = BigDecimal::new(
        (minted - CYCLES_LEDGER_DEPOSIT_FEE).into(), // digits
        12,                                          // scale
    );

    let balance = BigDecimal::new(
        balance.into(), // digits
        12,             // scale
    );

    // Output information
    let _ = ctx.term.write_line(&format!(
        "Minted {amount} trillion cycles, current balance: {balance} trillion cycles.",
    ));

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },

    #[snafu(display("ICP amount overflow. Specify less tokens."))]
    IcpAmountOverflow,

    #[snafu(display("failed to get identity principal: {err}"))]
    GetPrincipal { err: String },

    #[snafu(transparent)]
    Agent { source: AgentError },

    #[snafu(transparent)]
    Candid { source: candid::Error },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("transfer failed: {err}"))]
    Transfer { err: String },

    #[snafu(display(
        "insufficient funds. balance: {balance} {symbol}, required: {required} {symbol}"
    ))]
    InsufficientFunds {
        symbol: String,
        balance: BigDecimal,
        required: BigDecimal,
    },

    #[snafu(display("notification failed: {err}"))]
    Notify { err: String },
}
