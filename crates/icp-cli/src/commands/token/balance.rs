use clap::Args;
use icp::{agent, context::GetAgentError, identity, network};

use icp::context::Context;

use crate::commands::args::TokenCommandArgs;
use crate::operations::token::balance::{GetBalanceError, get_balance};

#[derive(Args, Clone, Debug)]
pub(crate) struct BalanceArgs {
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
    GetBalance(#[from] GetBalanceError),
}

/// Check the token balance of a given identity
///
/// The balance is checked against a ledger canister. Support two user flows:
/// (1) Specifying token name, and checking against known or stored mappings
/// (2) Specifying compatible ledger canister id
pub(crate) async fn exec(
    ctx: &Context,
    token: &str,
    args: &BalanceArgs,
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
    let balance_info = get_balance(&agent, token).await?;

    // Output information
    let _ = ctx.term.write_line(&format!(
        "Balance: {} {}",
        balance_info.amount, balance_info.symbol
    ));

    Ok(())
}
