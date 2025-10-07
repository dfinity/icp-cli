use bigdecimal::BigDecimal;
use candid::{Decode, Encode, Nat};
use clap::Parser;
use ic_agent::AgentError;
use icp_canister_interfaces::cycles_ledger::{
    CYCLES_LEDGER_DECIMALS, CYCLES_LEDGER_PRINCIPAL, WithdrawArgs, WithdrawError, WithdrawResponse,
};
use snafu::Snafu;

use crate::{
    context::{Context, ContextAgentError, ContextProjectError},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Debug, Parser)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: String,

    /// Amount of cycles to top up
    #[arg(long)]
    pub amount: u128,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest
    let pm = ctx.project()?;

    // Select canister to top up
    let (_, c) = pm
        .canisters
        .iter()
        .find(|(_, c)| cmd.name == c.name)
        .ok_or(CommandError::CanisterNotFound { name: cmd.name })?;

    // Load target environment
    let env = pm
        .environments
        .iter()
        .find(|&v| v.name == cmd.environment.name())
        .ok_or(CommandError::EnvironmentNotFound {
            name: cmd.environment.name().to_owned(),
        })?;

    // Collect environment canisters
    let ecs = env.canisters.clone().unwrap_or(
        pm.canisters
            .iter()
            .map(|(_, c)| c.name.to_owned())
            .collect(),
    );

    // Ensure canister is included in the environment
    if !ecs.contains(&c.name) {
        return Err(CommandError::EnvironmentCanister {
            environment: env.name.to_owned(),
            canister: c.name.to_owned(),
        });
    }

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

    // Lookup the canister id
    let cid = ctx.id_store.lookup(&Key {
        network: network.to_owned(),
        environment: env.name.to_owned(),
        canister: c.name.to_owned(),
    })?;

    // Load identity
    ctx.require_identity(cmd.identity.name());

    // Setup network
    ctx.require_network(network);

    // Prepare agent
    let agent = ctx.agent()?;

    let response_bytes = agent
        .update(&CYCLES_LEDGER_PRINCIPAL, "withdraw")
        .with_arg(
            Encode!(&WithdrawArgs {
                amount: Nat::from(cmd.amount),
                from_subaccount: None,
                to: cid,
                created_at_time: None,
            })
            .expect("Failed to encode WithdrawArgs"),
        )
        .call_and_wait()
        .await
        .map_err(|source| CommandError::Agent { source })?;
    let response =
        Decode!(&response_bytes, WithdrawResponse).expect("Failed to decode WithdrawResponse");
    response.map_err(|err| CommandError::Withdraw {
        err,
        amount: cmd.amount,
    })?;

    let _ = ctx.term.write_line(&format!(
        "Topped up canister {} with {}T cycles",
        c.name,
        BigDecimal::new(cmd.amount.into(), CYCLES_LEDGER_DECIMALS)
    ));

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: ContextProjectError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(display("environment '{environment}' does not include canister '{canister}'"))]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    GetAgent { source: ContextAgentError },

    #[snafu(transparent)]
    Agent { source: AgentError },

    #[snafu(display("Failed to top up: {}", err.format_error(*amount)))]
    Withdraw { err: WithdrawError, amount: u128 },
}
