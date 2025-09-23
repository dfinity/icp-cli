use bigdecimal::BigDecimal;
use candid::{Decode, Encode, Nat, Principal};
use clap::Parser;
use ic_agent::AgentError;
use icp_identity::key::LoadIdentityInContextError;
use icrc_ledger_types::icrc1::account::Account;
use snafu::Snafu;

use crate::{
    commands::token::TOKEN_LEDGER_CIDS,
    context::{Context, ContextAgentError, ContextProjectError},
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Debug, Parser)]
pub struct Cmd {
    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,
}

/// Check the token balance of a given identity
///
/// The balance is checked against a ledger canister. Support two user flows:
/// (1) Specifying token name, and checking against known or stored mappings
/// (2) Specifying compatible ledger canister id
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
    let (balance, decimals, symbol) = tokio::join!(
        //
        // Obtain token balance
        async {
            // Load identity
            let id = ctx.identity()?;

            // Convert identity to sender principal
            let owner = id
                .sender()
                .map_err(|err| CommandError::GetPrincipal { err })?;

            // Specify sub-account
            let subaccount = None;

            // Perform query
            let resp = agent
                .query(&cid, "icrc1_balance_of")
                .with_arg(Encode!(&Account { owner, subaccount }).expect("failed to encode arg"))
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
    let (Nat(balance), decimals, symbol) = (
        balance?,         //
        decimals? as i64, //
        symbol?,          //
    );

    // Calculate amount
    let amount = BigDecimal::from_biguint(balance, decimals);

    // Output information
    let _ = ctx.term.write_line(&format!("Balance: {amount} {symbol}"));

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: ContextProjectError },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    GetAgent { source: ContextAgentError },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("failed to get identity principal: {err}"))]
    GetPrincipal { err: String },

    #[snafu(transparent)]
    Agent { source: AgentError },

    #[snafu(transparent)]
    Candid { source: candid::Error },
}
