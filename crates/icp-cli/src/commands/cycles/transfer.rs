use candid::Principal;
use clap::Args;
use icp::context::Context;
use icp_canister_interfaces::cycles_ledger::{CYCLES_LEDGER_BLOCK_FEE, CYCLES_LEDGER_PRINCIPAL};

use crate::commands::args::TokenCommandArgs;
use crate::commands::parsers::parse_cycles_amount;
use crate::operations::token::transfer::icrc1_transfer;

#[derive(Debug, Args)]
pub(crate) struct TransferArgs {
    /// Cycles amount to transfer.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(value_parser = parse_cycles_amount)]
    pub(crate) amount: u128,

    /// The receiver of the cycles transfer
    pub(crate) receiver: Principal,

    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,
}

pub(crate) async fn exec(ctx: &Context, args: &TransferArgs) -> Result<(), anyhow::Error> {
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
    let transfer_info = icrc1_transfer(
        &agent,
        CYCLES_LEDGER_PRINCIPAL,
        args.amount.into(),
        args.receiver,
        CYCLES_LEDGER_BLOCK_FEE.into(),
        0,
        "cycles".to_string(),
    )
    .await?;

    // Output information
    let _ = ctx.term.write_line(&format!(
        "Transferred {} to {} in block {}",
        transfer_info.transferred, transfer_info.receiver_display, transfer_info.block_index
    ));

    Ok(())
}
