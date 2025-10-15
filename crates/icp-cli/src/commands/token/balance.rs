use bigdecimal::BigDecimal;
use candid::{Decode, Encode, Nat, Principal};
use clap::Parser;
use ic_agent::AgentError;
use icp::{agent, identity, network};
use icrc_ledger_types::icrc1::account::Account;
use tracing::info;

use crate::{
    commands::{Context, token::TOKEN_LEDGER_CIDS},
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Debug, Parser)]
pub struct Cmd {
    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,
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

    #[error("failed to get identity principal: {err}")]
    Principal { err: String },

    #[error(transparent)]
    Query(#[from] AgentError),

    #[error(transparent)]
    Candid(#[from] candid::Error),
}

/// Check the token balance of a given identity
///
/// The balance is checked against a ledger canister. Support two user flows:
/// (1) Specifying token name, and checking against known or stored mappings
/// (2) Specifying compatible ledger canister id
pub async fn exec(ctx: &Context, token: &str, cmd: Cmd) -> Result<(), CommandError> {
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

    // Obtain ledger address
    let cid = match TOKEN_LEDGER_CIDS.get(token) {
        // Given token matched known token names
        Some(cid) => cid.to_string(),

        // Given token is not known, indicating it's either already a canister id
        // or is simply a name of a token we do not know of
        None => token.to_string(),
    };

    // Parse the canister id
    let cid = Principal::from_text(cid).map_err(|err| CommandError::Principal {
        err: err.to_string(),
    })?;

    // Perform the required ledger calls
    let (balance, decimals, symbol) = tokio::join!(
        //
        // Obtain token balance
        async {
            // Convert identity to sender principal
            let owner = id.sender().map_err(|err| CommandError::Principal { err })?;

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
    info!("Balance: {amount} {symbol}");

    Ok(())
}
