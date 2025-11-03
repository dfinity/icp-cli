use bigdecimal::BigDecimal;
use candid::{Decode, Encode, Nat};
use clap::Args;
use ic_agent::AgentError;
use icp::{agent, context::GetCanisterIdAndAgentError, identity, network};
use icp_canister_interfaces::cycles_ledger::{
    CYCLES_LEDGER_DECIMALS, CYCLES_LEDGER_PRINCIPAL, WithdrawArgs, WithdrawError, WithdrawResponse,
};

use icp::context::Context;

use crate::commands::args;
use icp::store_id::LookupIdError;

#[derive(Debug, Args)]
pub(crate) struct TopUpArgs {
    /// Amount of cycles to top up
    #[arg(long)]
    pub(crate) amount: u128,

    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error(transparent)]
    Lookup(#[from] LookupIdError),

    #[error(transparent)]
    Update(#[from] AgentError),

    #[error(transparent)]
    Candid(#[from] candid::Error),

    #[error("Failed to top up: {}", err.format_error(*amount))]
    Withdraw { err: WithdrawError, amount: u128 },

    #[error(transparent)]
    GetCanisterIdAndAgent(#[from] GetCanisterIdAndAgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &TopUpArgs) -> Result<(), CommandError> {
    let selections = args.cmd_args.selections();
    let (cid, agent) = ctx
        .get_canister_id_and_agent(
            &selections.canister,
            &selections.environment,
            &selections.network,
            &selections.identity,
        )
        .await?;

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
        args.cmd_args.canister,
        BigDecimal::new(args.amount.into(), CYCLES_LEDGER_DECIMALS)
    ));

    Ok(())
}
