use clap::Args;
use ic_agent::AgentError;
use icp::{agent, identity, network};

use icp::context::{Context, GetAgentError, GetCanisterIdError};

use crate::commands::args;
use icp::store_id::LookupIdError;

#[derive(Debug, Args)]
pub(crate) struct StopArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
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
    LookupCanisterId(#[from] LookupIdError),

    #[error(transparent)]
    Stop(#[from] AgentError),

    #[error(transparent)]
    GetAgent(#[from] GetAgentError),

    #[error(transparent)]
    GetCanisterId(#[from] GetCanisterIdError),
}

pub(crate) async fn exec(ctx: &Context, args: &StopArgs) -> Result<(), CommandError> {
    let selections = args.cmd_args.selections();
    let agent = ctx
        .get_agent(
            &selections.environment,
            &selections.network,
            &selections.identity,
        )
        .await?;
    let cid = ctx
        .get_canister_id(
            &selections.canister,
            &selections.environment,
            &selections.network,
        )
        .await?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Instruct management canister to stop canister
    mgmt.stop_canister(&cid).await?;

    Ok(())
}
