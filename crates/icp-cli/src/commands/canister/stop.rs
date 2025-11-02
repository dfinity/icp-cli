use clap::Args;
use ic_agent::AgentError;

use icp::context::Context;

use crate::commands::args;

#[derive(Debug, Args)]
pub(crate) struct StopArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Stop(#[from] AgentError),

    #[error(transparent)]
    GetCanisterIdAndAgent(#[from] icp::context::GetCanisterIdAndAgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &StopArgs) -> Result<(), CommandError> {
    let selections = args.cmd_args.selections();

    let (cid, agent) = ctx
        .get_canister_id_and_agent(
            &selections.canister,
            &selections.environment,
            &selections.network,
            &selections.identity,
        )
        .await?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Instruct management canister to stop canister
    mgmt.stop_canister(&cid).await?;

    Ok(())
}
