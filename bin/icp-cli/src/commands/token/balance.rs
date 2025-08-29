use bigdecimal::BigDecimal;
use candid::{Decode, Encode, Nat};
use clap::Parser;
use ic_agent::AgentError;
use icp_identity::key::LoadIdentityInContextError;
use icrc_ledger_types::icrc1::account::Account;
use snafu::Snafu;

use crate::{
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Debug, Parser)]
pub struct Cmd {
    #[clap(flatten)]
    pub environment: EnvironmentOpt,

    #[clap(flatten)]
    pub identity: IdentityOpt,
    // todo: subaccount, owner, account_id
}

pub async fn exec(
    ctx: &Context,
    parent_cmd: super::TokenArgs,
    cmd: Cmd,
) -> Result<(), CommandError> {
    // Load identity
    ctx.require_identity(cmd.identity.name());

    // Load the project manifest, which defines the canisters to be built.
    let pm = ctx.project()?;
    println!("pm.networks: {:?}", pm.networks);
    println!("pm.environments: {:?}", pm.environments);

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
    println!("required network: {:?}", network);

    // Prepare agent
    let agent = ctx.agent()?;

    let token_address = parent_cmd
        .token_address()
        .ok_or(CommandError::InvalidToken {
            token: parent_cmd.token().to_string(),
        })?;

    let account = Account {
        owner: ctx
            .identity()?
            .as_ref()
            .sender()
            .map_err(|message| CommandError::GetPrincipalError { message })?,
        subaccount: None,
    };

    let balance_future = agent
        .query(&token_address, "icrc1_balance_of")
        .with_arg(Encode!(&account).unwrap())
        .call();
    let decimals_future = agent
        .query(&token_address, "icrc1_decimals")
        .with_arg(Encode!(&()).unwrap())
        .call();
    let symbol_future = agent
        .query(&token_address, "icrc1_symbol")
        .with_arg(Encode!(&()).unwrap())
        .call();

    let (balance, decimals, symbol) = tokio::join!(balance_future, decimals_future, symbol_future);
    let balance_bytes = balance
        .map_err(|e| CommandError::TokenCanisterError { source: e })
        .unwrap();
    let decimals_bytes = decimals
        .map_err(|e| CommandError::TokenCanisterError { source: e })
        .unwrap();
    let symbol_bytes = symbol
        .map_err(|e| CommandError::TokenCanisterError { source: e })
        .unwrap();

    let balance = Decode!(&balance_bytes, Nat).unwrap();
    let decimals = Decode!(&decimals_bytes, u8).unwrap();
    let symbol = Decode!(&symbol_bytes, String).unwrap();

    print_balance(balance, decimals, symbol);
    Ok(())
}

fn print_balance(balance: Nat, decimals: u8, symbol: String) {
    let amount = BigDecimal::from_biguint(balance.0, decimals as i64);
    println!("Balance: {amount} {symbol}");
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },

    #[snafu(display("Failed to get identity principal: {message}"))]
    GetPrincipalError { message: String },

    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("Token name unknown: {token}"))]
    InvalidToken { token: String },

    #[snafu(transparent)]
    LoadIdentity { source: LoadIdentityInContextError },

    #[snafu(display("Failed to talk to token canister: {source}"))]
    TokenCanisterError { source: AgentError },
}
