use clap::Args;
use ic_management_canister_types::DeleteCanisterSnapshotArgs;

use crate::commands::{args, canister::snapshot::SnapshotId};
use icp::context::Context;

#[derive(Debug, Args)]
pub struct DeleteArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// The ID of the snapshot to delete.
    snapshot: SnapshotId,
}

pub async fn exec(ctx: &Context, args: &DeleteArgs) -> Result<(), anyhow::Error> {
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

    // Delete snapshot
    mgmt.delete_canister_snapshot(
        &cid,
        &DeleteCanisterSnapshotArgs {
            canister_id: cid,
            snapshot_id: args.snapshot.0.clone(),
        },
    )
    .await?;

    eprintln!(
        "Deleted snapshot {} from canister '{}'",
        args.snapshot, args.cmd_args.canister,
    );

    Ok(())
}
