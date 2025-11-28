use clap::Args;
use icp::context::Context;

use crate::commands::args;

#[derive(Debug, Args)]
pub(crate) struct StopArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
}

pub(crate) async fn exec(ctx: &Context, args: &StopArgs) -> Result<(), anyhow::Error> {
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

    // Instruct management canister to stop canister
    mgmt.stop_canister(&cid).await?;

    Ok(())
}
