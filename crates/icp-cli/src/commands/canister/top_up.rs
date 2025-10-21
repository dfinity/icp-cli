use bigdecimal::BigDecimal;
use candid::{Decode, Encode, Nat};
use clap::Args;
use ic_agent::AgentError;
use icp::{agent, identity, network};
use icp_canister_interfaces::cycles_ledger::{
    CYCLES_LEDGER_DECIMALS, CYCLES_LEDGER_PRINCIPAL, WithdrawArgs, WithdrawError, WithdrawResponse,
};

use crate::{
    commands::{Context, Mode},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError},
};

#[derive(Debug, Args)]
pub(crate) struct TopUpArgs {
    /// The name of the canister within the current project
    pub(crate) name: String,

    /// Amount of cycles to top up
    #[arg(long)]
    pub(crate) amount: u128,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
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

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error(transparent)]
    Lookup(#[from] LookupError),

    #[error(transparent)]
    Update(#[from] AgentError),

    #[error(transparent)]
    Candid(#[from] candid::Error),

    #[error("Failed to top up: {}", err.format_error(*amount))]
    Withdraw { err: WithdrawError, amount: u128 },
}

pub(crate) async fn exec(ctx: &Context, args: &TopUpArgs) -> Result<(), CommandError> {
    // Load project
    let p = ctx.project.load().await?;

    // Load identity
    let id = ctx.identity.load(args.identity.clone().into()).await?;

    // Load target environment
    let env =
        p.environments
            .get(args.environment.name())
            .ok_or(CommandError::EnvironmentNotFound {
                name: args.environment.name().to_owned(),
            })?;

    // Access network
    let access = ctx.network.access(&env.network).await?;

    // Agent
    let agent = ctx.agent.create(id, &access.url).await?;

    if let Some(k) = access.root_key {
        agent.set_root_key(k);
    }

    // Ensure canister is included in the environment
    if !env.canisters.contains_key(&args.name) {
        return Err(CommandError::EnvironmentCanister {
            environment: env.name.to_owned(),
            canister: args.name.to_owned(),
        });
    }

    // Lookup the canister id
    let cid = ctx.ids.lookup(&Key {
        network: env.network.name.to_owned(),
        environment: env.name.to_owned(),
        canister: args.name.to_owned(),
    })?;

    let cargs = WithdrawArgs {
        amount: Nat::from(args.amount),
        from_subaccount: None,
        to: cid,
        created_at_time: None,
    };

    let bs = agent
        .update(&CYCLES_LEDGER_PRINCIPAL, "withdraw")
        .with_arg(Encode!(&cargs)?)
        .call_and_wait()
        .await?;

    Decode!(&bs, WithdrawResponse)?.map_err(|err| CommandError::Withdraw {
        err,
        amount: args.amount,
    })?;

    let _ = ctx.term.write_line(&format!(
        "Topped up canister {} with {}T cycles",
        args.name,
        BigDecimal::new(args.amount.into(), CYCLES_LEDGER_DECIMALS)
    ));

    Ok(())
}
