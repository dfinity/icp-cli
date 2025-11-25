use icp::context::Context;

use crate::commands::token;
use crate::operations::token::balance::get_balance;

pub(crate) async fn exec(
    ctx: &Context,
    args: &token::balance::BalanceArgs,
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
    let balance_info = get_balance(&agent, "cycles").await?;

    // Output information
    let _ = ctx.term.write_line(&format!(
        "Balance: {} {}",
        balance_info.amount, balance_info.symbol
    ));

    Ok(())
}
