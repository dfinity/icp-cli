use std::io::stdout;

use candid::Principal;
use clap::Args;
use icp::context::Context;
use icrc_ledger_types::icrc1::account::Account;
use serde::Serialize;

use crate::commands::args::TokenCommandArgs;
use crate::commands::parsers::parse_subaccount;
use crate::operations::token::allowance::get_allowance;

/// Display the allowance granted to a spender (ICRC-2) (default token: icp)
///
/// This is a read-only query that works for any owner/spender pair, including
/// accounts you do not control (use `--of-principal` to set the owner). The amount
/// is shown in whole tokens, along with an expiry if one was set. Works with any
/// ICRC-2 ledger, referenced by a known token name or a ledger canister id.
#[derive(Args, Debug)]
#[command(override_usage = "icp token [TOKEN|LEDGER_ID] allowance [OPTIONS] <SPENDER>")]
pub(crate) struct AllowanceArgs {
    /// Principal of the spender whose allowance to look up.
    pub(crate) spender: Principal,

    /// The spender's subaccount, as a hex string (32 bytes, left-padded).
    /// Defaults to the default subaccount.
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) spender_subaccount: Option<[u8; 32]>,

    /// The owner's subaccount that granted the allowance, as a hex string
    /// (32 bytes, left-padded). Defaults to the default subaccount.
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) subaccount: Option<[u8; 32]>,

    /// The allowance owner to look up, instead of the current identity.
    /// Lets you inspect allowances granted by any principal.
    #[arg(long)]
    pub(crate) of_principal: Option<Principal>,

    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,

    /// Output command results as JSON
    #[arg(long, conflicts_with = "quiet")]
    pub(crate) json: bool,

    /// Suppress human-readable output; print only the allowance amount
    #[arg(long, short)]
    pub(crate) quiet: bool,
}

/// Display the allowance an owner account has granted to a spender
///
/// The allowance is queried against an ICRC-2 compatible ledger canister. Supports
/// two user flows:
/// (1) Specifying token name, and checking against known or stored mappings
/// (2) Specifying compatible ledger canister id
pub(crate) async fn exec(
    ctx: &Context,
    token: &str,
    args: &AllowanceArgs,
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
    let owner = args
        .of_principal
        .unwrap_or_else(|| agent.get_principal().unwrap());

    let spender = Account {
        owner: args.spender,
        subaccount: args.spender_subaccount,
    };

    // Query the allowance from the ledger
    let info = get_allowance(&agent, token, owner, args.subaccount, spender).await?;

    if args.json {
        serde_json::to_writer(
            stdout(),
            &JsonAllowance {
                allowance: info.allowance.to_string(),
                expires_at: info.expires_at,
            },
        )?;
    } else if args.quiet {
        println!("{}", info.allowance);
    } else {
        println!("Allowance: {}", info.allowance);
        if let Some(expires_at) = info.expires_at {
            println!("Expires at: {expires_at} (nanoseconds since epoch)");
        }
    }

    Ok(())
}

#[derive(Serialize)]
struct JsonAllowance {
    allowance: String,
    expires_at: Option<u64>,
}
