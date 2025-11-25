use anyhow::{Context as _, bail};
use bigdecimal::BigDecimal;
use candid::{Decode, Encode, Nat};
use clap::Args;
use icp::context::Context;
use icp_canister_interfaces::cycles_ledger::{
    CYCLES_LEDGER_DECIMALS, CYCLES_LEDGER_PRINCIPAL, WithdrawArgs, WithdrawResponse,
};

use crate::commands::args;

#[derive(Debug, Args)]
pub(crate) struct TopUpArgs {
    /// Amount of cycles to top up
    #[arg(long)]
    pub(crate) amount: u128,

    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
}

pub(crate) async fn exec(ctx: &Context, args: &TopUpArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();
    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;
    let cid = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
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

    let response = Decode!(&bs, WithdrawResponse).context("failed to decode withdraw response")?;
    if let Err(err) = response {
        bail!("failed to top up: {}", err.format_error(args.amount));
    }

    let _ = ctx.term.write_line(&format!(
        "Topped up canister {} with {}T cycles",
        args.cmd_args.canister,
        BigDecimal::new(args.amount.into(), CYCLES_LEDGER_DECIMALS)
    ));

    Ok(())
}
