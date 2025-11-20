use bigdecimal::BigDecimal;
use candid::Principal;
use clap::Args;
use icp::{agent, context::GetAgentError, identity, network};

use icp::context::Context;

use crate::commands::args::TokenCommandArgs;
use crate::operations::token::transfer::{transfer, TokenTransferError};

#[derive(Debug, Args)]
pub(crate) struct TransferArgs {
    /// Token amount to transfer
    pub(crate) amount: BigDecimal,

    /// The receiver of the token transfer
    pub(crate) receiver: Principal,

    #[command(flatten)]
    pub(crate) token_command_args: TokenCommandArgs,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateAgentError),

    #[error(transparent)]
    GetAgent(#[from] GetAgentError),

    #[error(transparent)]
    Transfer(#[from] TokenTransferError),
}

pub(crate) async fn exec(
    ctx: &Context,
    token: &str,
    args: &TransferArgs,
) -> Result<(), CommandError> {
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
    let transfer_info = transfer(&agent, token, &args.amount, args.receiver).await?;

    // Output information
    let _ = ctx.term.write_line(&format!(
        "Transferred {} {} to {} in block {}",
        transfer_info.amount, transfer_info.symbol, transfer_info.receiver, transfer_info.block_index
    ));

    Ok(())
}
