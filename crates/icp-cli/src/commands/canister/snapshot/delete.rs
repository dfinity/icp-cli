use candid::Principal;
use clap::Args;
use ic_management_canister_types::DeleteCanisterSnapshotArgs;
use icp::context::Context;
use tracing::info;

use super::SnapshotId;
use crate::{commands::args, operations::proxy_management};

/// Delete a canister snapshot
#[derive(Debug, Args)]
pub(crate) struct DeleteArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// The snapshot ID to delete (hex-encoded)
    snapshot_id: SnapshotId,

    /// Principal of a proxy canister to route the management canister call through.
    #[arg(long)]
    proxy: Option<Principal>,
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

    let delete_args = DeleteCanisterSnapshotArgs {
        canister_id: cid,
        snapshot_id: args.snapshot_id.0.clone(),
    };

    proxy_management::delete_canister_snapshot(&agent, args.proxy, delete_args).await?;

    let name = &args.cmd_args.canister;
    info!(
        "Deleted snapshot {id} from canister {name} ({cid})",
        id = hex::encode(&args.snapshot_id.0),
    );

    Ok(())
}
