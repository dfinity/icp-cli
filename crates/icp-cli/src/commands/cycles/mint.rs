use bigdecimal::BigDecimal;
use clap::Args;
use icp::{agent, context::GetAgentError, identity, network};

use icp::context::Context;

use crate::commands::args::TokenCommandArgs;
use crate::operations::token::mint::{mint_cycles, MintCyclesError};

#[derive(Debug, Args)]
pub(crate) struct MintArgs {
    /// Amount of ICP to mint to cycles.
    #[arg(long, conflicts_with = "cycles")]
    pub(crate) icp: Option<BigDecimal>,

    /// Amount of cycles to mint. Automatically determines the amount of ICP needed.
    #[arg(long, conflicts_with = "icp")]
    pub(crate) cycles: Option<u128>,

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
    MintCycles(#[from] MintCyclesError),

    #[error("No amount specified. Use --icp or --cycles.")]
    NoAmountSpecified,
}

pub(crate) async fn exec(ctx: &Context, args: &MintArgs) -> Result<(), CommandError> {
    // Validate args
    if args.icp.is_none() && args.cycles.is_none() {
        return Err(CommandError::NoAmountSpecified);
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
    let mint_info = mint_cycles(&agent, args.icp.as_ref(), args.cycles).await?;

    // Display results
    let _ = ctx.term.write_line(&format!(
        "Minted {} TCYCLES to your account, new balance: {} TCYCLES.",
        mint_info.deposited, mint_info.new_balance
    ));

    Ok(())
}
