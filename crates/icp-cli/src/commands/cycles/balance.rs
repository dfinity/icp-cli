use icp::{agent, context::GetAgentError, identity, network};
use icp::context::Context;

use crate::commands::token;
use crate::operations::token::balance::{get_balance, GetBalanceError};

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
    GetBalance(#[from] GetBalanceError),
}

pub(crate) async fn exec(
    ctx: &Context,
    args: &token::balance::BalanceArgs,
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

    // Get the balance from the ledger
    let balance_info = get_balance(&agent, "cycles").await?;

    // Output information
    let _ = ctx
        .term
        .write_line(&format!("Balance: {} {}", balance_info.amount, balance_info.symbol));

    Ok(())
}
