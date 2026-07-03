use std::io::stdout;

use bigdecimal::BigDecimal;
use candid::Principal;
use clap::Args;
use icp::context::Context;
use icp::parsers::parse_token_amount;
use icrc_ledger_types::icrc1::account::Account;
use serde::Serialize;

use crate::commands::args::TokenCommandArgs;
use crate::commands::parsers::parse_subaccount;
use crate::operations::token::approve::approve;

/// Approve a spender to transfer tokens on your behalf (ICRC-2) (default token: icp)
///
/// Sets the spender's allowance to the given amount, overwriting any existing
/// allowance (this is a set, not an increment). The allowance is granted from the
/// calling identity's account, which is charged the ledger's approval fee. Works
/// with any ICRC-2 ledger, referenced by a known token name or a ledger canister id.
#[derive(Debug, Args)]
#[command(override_usage = "icp token [TOKEN|LEDGER_ID] approve [OPTIONS] <AMOUNT> <SPENDER>")]
pub(crate) struct ApproveArgs {
    /// The allowance amount, in whole tokens (e.g. `1.5`), the spender may transfer.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(value_parser = parse_token_amount)]
    pub(crate) amount: BigDecimal,

    /// Principal of the spender being granted the allowance.
    pub(crate) spender: Principal,

    /// The spender's subaccount, as a hex string (32 bytes, left-padded).
    /// Defaults to the default subaccount.
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) spender_subaccount: Option<[u8; 32]>,

    /// The caller's subaccount to grant the allowance from (the account debited),
    /// as a hex string (32 bytes, left-padded). Defaults to the default subaccount.
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) from_subaccount: Option<[u8; 32]>,

    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,

    /// Output command results as JSON
    #[arg(long, conflicts_with = "quiet")]
    pub(crate) json: bool,

    /// Suppress human-readable output; print only the block index
    #[arg(long, short)]
    pub(crate) quiet: bool,
}

/// Approve a spender to transfer tokens on the current identity's behalf
///
/// The allowance is set against an ICRC-2 compatible ledger canister. Supports two
/// user flows:
/// (1) Specifying token name, and checking against known or stored mappings
/// (2) Specifying compatible ledger canister id
pub(crate) async fn exec(
    ctx: &Context,
    token: &str,
    args: &ApproveArgs,
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

    let spender = Account {
        owner: args.spender,
        subaccount: args.spender_subaccount,
    };

    // Execute approve
    let info = approve(&agent, token, &args.amount, args.from_subaccount, spender).await?;

    if args.json {
        serde_json::to_writer(
            stdout(),
            &JsonApprove {
                block_index: info.block_index.to_string(),
            },
        )?;
    } else if args.quiet {
        println!("{}", info.block_index);
    } else {
        println!(
            "Approved {} to spend up to {} (block {})",
            info.spender_display, info.allowance, info.block_index
        );
    }

    Ok(())
}

#[derive(Serialize)]
struct JsonApprove {
    block_index: String,
}
