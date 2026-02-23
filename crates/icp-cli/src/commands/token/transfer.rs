use bigdecimal::BigDecimal;
use clap::Args;
use icp::context::Context;

use crate::commands::args::{FlexibleAccountId, TokenCommandArgs};
use crate::commands::parsers::{parse_subaccount, parse_token_amount};
use crate::operations::token::transfer::transfer;

/// Transfer ICP or ICRC1 tokens through their ledger (default token: icp)
#[derive(Debug, Args)]
#[command(override_usage = "icp token [TOKEN|LEDGER_ID] transfer [OPTIONS] <AMOUNT> <RECEIVER>")]
pub(crate) struct TransferArgs {
    /// Token amount to transfer.
    /// Supports suffixes: k (thousand), m (million), b (billion), t (trillion).
    #[arg(value_parser = parse_token_amount)]
    pub(crate) amount: BigDecimal,

    /// The receiver of the token transfer.
    /// Can be a principal, an ICRC1 account ID, or an ICP ledger account ID (hex).
    pub(crate) receiver: FlexibleAccountId,

    /// The subaccount to transfer to (only if the receiver is a principal)
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) to_subaccount: Option<[u8; 32]>,

    /// The subaccount to transfer from
    #[arg(long, value_parser = parse_subaccount)]
    pub(crate) from_subaccount: Option<[u8; 32]>,

    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,
}

pub(crate) async fn exec(
    ctx: &Context,
    token: &str,
    args: &TransferArgs,
) -> Result<(), anyhow::Error> {
    let mut receiver = args.receiver;
    if let Some(subaccount) = args.to_subaccount {
        if let FlexibleAccountId::Icrc1(account) = &mut receiver
            && account.subaccount.is_none()
        {
            account.subaccount = Some(subaccount);
        } else {
            return Err(anyhow::anyhow!(
                "Cannot use --to-subaccount with an account ID. Use a plain principal if you want to specify a subaccount."
            ));
        }
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
    let transfer_info =
        transfer(&agent, args.from_subaccount, token, &args.amount, &receiver).await?;

    // Output information
    let _ = ctx.term.write_line(&format!(
        "Transferred {} to {} in block {}",
        transfer_info.transferred, transfer_info.receiver_display, transfer_info.block_index
    ));

    Ok(())
}
