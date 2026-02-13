use clap::Args;
use icp::context::{CanisterSelection, Context};

use crate::commands::args;

/// Delete a canister from a network
#[derive(Debug, Args)]
pub(crate) struct DeleteArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
}

pub(crate) async fn exec(ctx: &Context, args: &DeleteArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();

    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;
    let cid = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
        )
        .await?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Instruct management canister to delete canister
    mgmt.delete_canister(&cid).await?;

    // Remove canister ID from the id store if it was referenced by name
    if let CanisterSelection::Named(canister_name) = &selections.canister {
        ctx.remove_canister_id_for_env(canister_name, &selections.environment)
            .await?;
    }

    Ok(())
}
