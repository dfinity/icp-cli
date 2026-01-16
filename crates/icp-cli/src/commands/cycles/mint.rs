use anyhow::bail;
use bigdecimal::BigDecimal;
use clap::Args;
use icp::context::Context;

use crate::commands::args::TokenCommandArgs;
use crate::operations::token::mint::mint_cycles;

#[derive(Debug, Args)]
pub(crate) struct MintArgs {
    /// Amount of ICP to mint to cycles.
    #[arg(long, conflicts_with = "tcycles")]
    pub(crate) icp: Option<BigDecimal>,

    /// Amount of cycles to mint (in TCYCLES). Automatically determines the amount of ICP needed.
    #[arg(long, conflicts_with = "icp")]
    pub(crate) tcycles: Option<BigDecimal>,

    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,
}

pub(crate) async fn exec(ctx: &Context, args: &MintArgs) -> Result<(), anyhow::Error> {
    // Validate args
    if args.icp.is_none() && args.tcycles.is_none() {
        bail!("no amount specified. Use --icp or --tcycles");
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
    let mint_info = mint_cycles(&agent, args.icp.as_ref(), args.tcycles.as_ref()).await?;

    // Display results
    let _ = ctx.term.write_line(&format!(
        "Minted {} TCYCLES to your account, new balance: {} TCYCLES.",
        mint_info.deposited, mint_info.new_balance
    ));

    Ok(())
}
