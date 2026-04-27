use std::io::stdout;

use candid::Principal;
use clap::Args;
use icp::context::Context;
use serde::Serialize;

use crate::commands::args::TokenCommandArgs;
use crate::commands::parsers::parse_subaccount;
use crate::operations::token::balance::get_balance;

/// Display the token balance on the ledger (default token: icp)
#[derive(Args, Clone, Debug)]
#[command(override_usage = "icp token [TOKEN|LEDGER_ID] balance [OPTIONS]")]
pub(crate) struct BalanceArgs {
    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,

    /// The subaccount to check the balance for
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) subaccount: Option<[u8; 32]>,

    /// Check the balance of this principal instead of the current identity
    #[arg(long)]
    pub(crate) of_principal: Option<Principal>,

    /// Output command results as JSON
    #[arg(long, conflicts_with = "quiet")]
    pub(crate) json: bool,

    /// Suppress human-readable output; print only the balance
    #[arg(long, short)]
    pub(crate) quiet: bool,
}

/// Check the token balance of a given identity
///
/// The balance is checked against a ledger canister. Support two user flows:
/// (1) Specifying token name, and checking against known or stored mappings
/// (2) Specifying compatible ledger canister id
pub(crate) async fn exec(
    ctx: &Context,
    token: &str,
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
    let balance = get_balance(&agent, args.subaccount, token, args.of_principal).await?;

    if args.json {
        serde_json::to_writer(
            stdout(),
            &JsonBalance {
                balance: balance.to_string(),
            },
        )?;
    } else if args.quiet {
        println!("{balance}");
    } else {
        println!("Balance: {balance}");
    }

    Ok(())
}

#[derive(Serialize)]
struct JsonBalance {
    balance: String,
}
