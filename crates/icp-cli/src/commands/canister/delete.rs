use clap::Args;
use ic_agent::AgentError;

use icp::context::{CanisterSelection, Context, EnvironmentSelection, NetworkSelection};
use icp::identity::IdentitySelection;

use crate::commands::args;

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
    GetCanisterIdAndAgent(#[from] icp::context::GetCanisterIdAndAgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &DeleteArgs) -> Result<(), CommandError> {
    let canister_selection: CanisterSelection = args.cmd_args.canister.clone().into();
    let environment_selection: EnvironmentSelection =
        args.cmd_args.environment.clone().unwrap_or_default().into();
    let network_selection: NetworkSelection = match args.cmd_args.network.clone() {
        Some(network) => network.into_selection(),
        None => NetworkSelection::FromEnvironment,
    };
    let identity_selection: IdentitySelection = args.cmd_args.identity.clone().into();

    let (cid, agent) = ctx
        .get_canister_id_and_agent(
            &canister_selection,
            &environment_selection,
            &network_selection,
            &identity_selection,
        )
        .await?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Instruct management canister to delete canister
    mgmt.delete_canister(&cid).await?;

    // TODO(or.ricon): Remove the canister association with the network/environment

    Ok(())
}
