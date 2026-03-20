use std::io::stdout;

use anyhow::bail;
use bigdecimal::BigDecimal;
use clap::Args;
use icp::context::Context;
use icp::parsers::{CyclesAmount, parse_token_amount};
use serde::Serialize;

use crate::commands::args::TokenCommandArgs;
use crate::commands::parsers::parse_subaccount;
use crate::operations::token::mint::mint_cycles;

/// Convert icp to cycles
#[derive(Debug, Args)]
pub(crate) struct MintArgs {
    /// Amount of ICP to mint to cycles.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(long, conflicts_with = "cycles", value_parser = parse_token_amount)]
    pub(crate) icp: Option<BigDecimal>,

    /// Amount of cycles to mint. Automatically determines the amount of ICP needed.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(long, conflicts_with = "icp")]
    pub(crate) cycles: Option<CyclesAmount>,

    /// Subaccount to withdraw the ICP from.
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) from_subaccount: Option<[u8; 32]>,

    /// Subaccount to deposit the cycles to.
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) to_subaccount: Option<[u8; 32]>,

    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,

    /// Output command results as JSON
    #[arg(long)]
    pub(crate) json: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &MintArgs) -> Result<(), anyhow::Error> {
    // Validate args
    if args.icp.is_none() && args.cycles.is_none() {
        bail!("no amount specified. Use --icp or --cycles");
    }

    let selections = args.token_command_args.selections();

    // Agent
    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;

    // Execute mint operation
    let mint_info = mint_cycles(
        &agent,
        args.icp.as_ref(),
        args.cycles.as_ref().map(|c| c.get()),
        args.from_subaccount,
        args.to_subaccount,
    )
    .await?;

    if args.json {
        serde_json::to_writer(
            stdout(),
            &JsonMint {
                deposited: mint_info.deposited.to_string(),
                new_balance: mint_info.new_balance.to_string(),
            },
        )?;
    } else {
        println!(
            "Minted {} to your account, new balance: {}.",
            mint_info.deposited, mint_info.new_balance
        );
    }

    Ok(())
}

#[derive(Serialize)]
struct JsonMint {
    deposited: String,
    new_balance: String,
}
