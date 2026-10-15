use anyhow::{Context as _, bail};
use bigdecimal::BigDecimal;
use candid::{Decode, Encode, Nat};
use clap::Args;
use icp_app::context::Context;
use icp_canister_interfaces::cycles_ledger::{
    CYCLES_LEDGER_BLOCK_FEE, CYCLES_LEDGER_PRINCIPAL, WithdrawArgs, WithdrawError, WithdrawResponse,
};
use icp_project::parsers::CyclesAmount;
use num_traits::ToPrimitive;
use tracing::info;

use crate::commands::args;
use crate::shortfall;
use icp_app::operations::token::TokenAmount;

/// Top up a canister with cycles
#[derive(Debug, Args)]
pub(crate) struct TopUpArgs {
    /// Amount of cycles to top up.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(long)]
    pub(crate) amount: CyclesAmount,

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
        amount: Nat::from(args.amount.get()),
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
    // Refused for want of cycles: how many are missing, and how to get them
    // (see `shortfall`). A balance past u128 is not short of a u128 request.
    if let Err(WithdrawError::InsufficientFunds { balance }) = &response
        && let Some(hint) = shortfall::Hint::read(
            ctx,
            &agent,
            args.amount.get().saturating_add(CYCLES_LEDGER_BLOCK_FEE),
            balance.0.to_u128().unwrap_or(u128::MAX),
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await
    {
        bail!(hint.message(
            &format!(
                "top up {} with {}",
                args.cmd_args.canister,
                shortfall::cycles(args.amount.get())
            ),
            "run the top-up again",
            None,
        ));
    }
    if let Err(err) = response {
        bail!("failed to top up: {}", err.format_error(args.amount.get()));
    }

    let amount = TokenAmount {
        amount: BigDecimal::new(args.amount.get().into(), 0),
        symbol: "cycles".to_string(),
    };

    info!(
        "Topped up canister {} with {}",
        args.cmd_args.canister, amount
    );

    Ok(())
}
