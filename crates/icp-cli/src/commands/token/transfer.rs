use bigdecimal::BigDecimal;
use clap::Args;
use icp::context::Context;

use crate::commands::args::TokenCommandArgs;
use crate::commands::parsers::parse_token_amount;
use crate::operations::token::transfer::transfer;

#[derive(Debug, Args)]
pub(crate) struct TransferArgs {
    /// Token amount to transfer.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(value_parser = parse_token_amount)]
    pub(crate) amount: BigDecimal,

    /// The receiver of the token transfer.
    /// Can be a Principal or an AccountIdentifier hex string (only for ICP ledger).
    pub(crate) receiver: String,

    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,
}

pub(crate) async fn exec(
    ctx: &Context,
    token: &str,
    args: &TransferArgs,
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

    // Execute transfer
    let transfer_info = transfer(&agent, token, &args.amount, &args.receiver).await?;

    // Output information
    let _ = ctx.term.write_line(&format!(
        "Transferred {} to {} in block {}",
        transfer_info.transferred, transfer_info.receiver_display, transfer_info.block_index
    ));

    Ok(())
}
