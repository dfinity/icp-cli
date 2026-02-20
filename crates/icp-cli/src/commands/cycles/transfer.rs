use anyhow::ensure;
use clap::Args;
use icp::context::Context;
use icp::parsers::CyclesAmount;
use icp_canister_interfaces::cycles_ledger::{CYCLES_LEDGER_BLOCK_FEE, CYCLES_LEDGER_PRINCIPAL};
use icrc_ledger_types::icrc1::account::Account;

use crate::commands::args::TokenCommandArgs;
use crate::commands::parsers::parse_subaccount;
use crate::operations::token::transfer::icrc1_transfer;

/// Transfer cycles to another principal
#[derive(Debug, Args)]
pub(crate) struct TransferArgs {
    /// Cycles amount to transfer.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    pub(crate) amount: CyclesAmount,

    /// The receiver of the cycles transfer
    pub(crate) receiver: Account,

    /// The subaccount to transfer to (only if the receiver is a principal)
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) to_subaccount: Option<[u8; 32]>,

    //// The subaccount to transfer from
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) from_subaccount: Option<[u8; 32]>,

    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,
}

pub(crate) async fn exec(ctx: &Context, args: &TransferArgs) -> Result<(), anyhow::Error> {
    ensure!(
        !(args.to_subaccount.is_some() && args.receiver.subaccount.is_some()),
        "Cannot use both --subaccount with an account ID. Use a plain principal if you want to change the subaccount."
    );
    let mut receiver = args.receiver;
    if let Some(subaccount) = args.to_subaccount {
        receiver.subaccount = Some(subaccount);
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

    // Execute transfer
    let transfer_info = icrc1_transfer(
        &agent,
        args.from_subaccount,
        CYCLES_LEDGER_PRINCIPAL,
        args.amount.get().into(),
        receiver,
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
