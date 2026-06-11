use candid::Principal;
use clap::Args;
use ic_management_canister_types::CanisterIdRecord;
use icp::context::Context;

use crate::{commands::args, operations::proxy_management};

/// Stop a canister on a network
#[derive(Debug, Args)]
pub(crate) struct StopArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Principal of a proxy canister to route the management canister call through.
    #[arg(long)]
    pub(crate) proxy: Option<Principal>,
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

    proxy_management::stop_canister(&agent, args.proxy, CanisterIdRecord { canister_id: cid })
        .await?;

    Ok(())
}
