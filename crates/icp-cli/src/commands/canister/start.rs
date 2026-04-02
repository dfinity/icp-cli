use clap::Args;
use ic_management_canister_types::CanisterIdRecord;
use icp::context::Context;

use crate::{commands::args, operations::proxy_management};

/// Start a canister on a network
#[derive(Debug, Args)]
pub(crate) struct StartArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
}

pub(crate) async fn exec(ctx: &Context, args: &StartArgs) -> Result<(), anyhow::Error> {
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

    proxy_management::start_canister(&agent, None, CanisterIdRecord { canister_id: cid }).await?;

    Ok(())
}
