use bigdecimal::{BigDecimal, num_bigint::ToBigInt};
use candid::{Decode, Encode, Nat, Principal};
use clap::Parser;
use ic_agent::AgentError;
use icp_identity::key::LoadIdentityInContextError;
use icrc_ledger_types::icrc1::{
    account::Account,
    transfer::{TransferArg, TransferError},
};
use snafu::Snafu;

use crate::{
    commands::token::TOKEN_LEDGER_CIDS,
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Debug, Parser)]
pub struct Cmd {
    /// Token amount to transfer
    pub amount: BigDecimal,

    /// The receiver of the token transfer
    pub receiver: Principal,

    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, token: &str, cmd: Cmd) -> Result<(), CommandError> {
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

    // Obtain ledger address
    let cid = match TOKEN_LEDGER_CIDS.get(token) {
        // Given token matched known token names
        Some(cid) => cid.to_string(),

        // Given token is not known, indicating it's either already a canister id
        // or is simply a name of a token we do not know of
        None => token.to_string(),
    };

    // Parse the canister id
    let cid = Principal::from_text(cid).map_err(|err| CommandError::GetPrincipal {
        err: err.to_string(),
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
                .await?;

            // Decode response
            Ok::<_, CommandError>(Decode!(&resp, Nat)?)
        },
        //
        // Obtain the number of decimals the token uses
        async {
            // Perform query
            let resp = agent
                .query(&cid, "icrc1_decimals")
                .with_arg(Encode!(&()).expect("failed to encode arg"))
                .await?;

            // Decode response
            Ok::<_, CommandError>(Decode!(&resp, u8)?)
        },
        //
        // Obtain the symbol of the token
        async {
            // Perform query
            let resp = agent
                .query(&cid, "icrc1_symbol")
                .with_arg(Encode!(&()).expect("failed to encode arg"))
                .await?;

            // Decode response
            Ok::<_, CommandError>(Decode!(&resp, String)?)
        },
    );

    // Check for errors
    let (Nat(fee), decimals, symbol) = (
        fee?,             //
        decimals? as u32, //
        symbol?,          //
    );

    // Calculate units of token to transfer
    let amount = cmd.amount.clone() * 10u128.pow(decimals);

    // Convert amount to big decimal
    let amount = amount
        .to_bigint()
        .ok_or(CommandError::Amount)?
        .to_biguint()
        .ok_or(CommandError::Amount)?;

    let amount = Nat::from(amount);

    // Prepare transfer
    let receiver = Account {
        owner: cmd.receiver,
        subaccount: None,
    };

    let arg = TransferArg {
        // Transfer amount
        amount: amount.clone(),

        // Transfer destination
        to: receiver,

        // Other
        from_subaccount: None,
        fee: None,
        created_at_time: None,
        memo: None,
    };

    // Perform transfer
    let resp = agent
        .update(&cid, "icrc1_transfer")
        .with_arg(Encode!(&arg)?)
        .call_and_wait()
        .await?;

    // Parse response
    let resp = Decode!(&resp, Result<Nat, TransferError>)?;

    // Process response
    let idx = resp.map_err(|err| match err {
        // Special case for insufficient funds
        TransferError::InsufficientFunds { balance } => {
            let balance = BigDecimal::from_biguint(
                balance.0,       // balance
                decimals as i64, // decimals
            );

            let fee = BigDecimal::from_biguint(
                fee,             // fee
                decimals as i64, // decimals
            );

            CommandError::InsufficientFunds {
                symbol: symbol.clone(),
                balance,
                required: cmd.amount + fee,
            }
        }

        _ => CommandError::Transfer {
            err: err.to_string(),
        },
    })?;

    // Output information
    let _ = ctx.term.write_line(&format!(
        "Transferred {amount} {symbol} to {receiver} in block {idx}"
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

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("failed to get identity principal: {err}"))]
    GetPrincipal { err: String },

    #[snafu(transparent)]
    Agent { source: AgentError },

    #[snafu(transparent)]
    Candid { source: candid::Error },

    #[snafu(display("invalid amount"))]
    Amount,

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
}
