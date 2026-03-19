use std::io::stdout;

use bigdecimal::BigDecimal;
use clap::Args;
use icp::context::Context;
use icp_canister_interfaces::cycles_ledger::CYCLES_LEDGER_PRINCIPAL;
use serde::Serialize;

use crate::commands::args::TokenCommandArgs;
use crate::commands::parsers::parse_subaccount;
use crate::operations::token::TokenAmount;
use crate::operations::token::balance::get_raw_balance;

/// Display the cycles balance
#[derive(Args, Clone, Debug)]
pub(crate) struct BalanceArgs {
    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,

    /// The subaccount to check the balance for
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) subaccount: Option<[u8; 32]>,

    /// Output command results as JSON
    #[arg(long, conflicts_with = "quiet")]
    pub(crate) json: bool,

    /// Suppress human-readable output; print only the balance
    #[arg(long, short)]
    pub(crate) quiet: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &BalanceArgs) -> Result<(), anyhow::Error> {
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
    let cycles = get_raw_balance(&agent, CYCLES_LEDGER_PRINCIPAL, args.subaccount).await?;
    let cycles_amount = TokenAmount {
        amount: BigDecimal::from_biguint(cycles.0, 0),
        symbol: "cycles".to_string(),
    };

    if args.json {
        serde_json::to_writer(
            stdout(),
            &JsonBalance {
                balance: cycles_amount.to_string(),
            },
        )?;
    } else if args.quiet {
        println!("{cycles_amount}");
    } else {
        println!("Balance: {cycles_amount}");
    }

    Ok(())
}

#[derive(Serialize)]
struct JsonBalance {
    balance: String,
}
