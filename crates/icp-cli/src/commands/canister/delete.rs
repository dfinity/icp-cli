use clap::Args;
use ic_agent::AgentError;

use crate::{
    commands::{args::{self, ArgValidationError}, Context},
};

#[derive(Debug, Args)]
pub(crate) struct DeleteArgs {

    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {

    #[error(transparent)]
    Delete(#[from] AgentError),

    #[error(transparent)]
    Shared(#[from] ArgValidationError),
}

pub(crate) async fn exec(ctx: &Context, args: &DeleteArgs) -> Result<(), CommandError> {

    let (cid, agent) = args.cmd_args.get_cid_and_agent(ctx).await?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Instruct management canister to delete canister
    mgmt.delete_canister(&cid).await?;

    // TODO(or.ricon): Remove the canister association with the network/environment

    Ok(())
}
