use anyhow::bail;
use bigdecimal::BigDecimal;
use clap::Args;
use icp::context::Context;
use icp::parsers::{parse_cycles_amount, parse_token_amount};

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
    #[arg(long, conflicts_with = "icp", value_parser = parse_cycles_amount)]
    pub(crate) cycles: Option<u128>,

    /// Subaccount to withdraw the ICP from.
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) from_subaccount: Option<[u8; 32]>,

    /// Subaccount to deposit the cycles to.
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) to_subaccount: Option<[u8; 32]>,

    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,
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
        args.cycles,
        args.from_subaccount,
        args.to_subaccount,
    )
    .await?;

    // Display results
    let _ = ctx.term.write_line(&format!(
        "Minted {} to your account, new balance: {}.",
        mint_info.deposited, mint_info.new_balance
    ));

    Ok(())
}
