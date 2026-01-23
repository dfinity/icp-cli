use bigdecimal::BigDecimal;
use clap::Args;
use icp::context::Context;
use icp_canister_interfaces::cycles_ledger::CYCLES_LEDGER_PRINCIPAL;

use crate::commands::args::TokenCommandArgs;
use crate::operations::token::TokenAmount;
use crate::operations::token::balance::get_raw_balance;

#[derive(Args, Clone, Debug)]
pub(crate) struct BalanceArgs {
    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,
}

pub(crate) async fn exec(
    ctx: &Context,
    args: &BalanceArgs,
) -> Result<(), anyhow::Error> {
    let selections = args.token_command_args.selections();

    // Agent
    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;

    // Get the balance from the ledger
    let cycles = get_raw_balance(&agent, CYCLES_LEDGER_PRINCIPAL).await?;
    let cycles_amount = TokenAmount {
        amount: BigDecimal::from_biguint(cycles.0, 0),
        symbol: "cycles".to_string(),
    };

    // Output information
    let _ = ctx.term.write_line(&format!("Balance: {cycles_amount}"));

    Ok(())
}
