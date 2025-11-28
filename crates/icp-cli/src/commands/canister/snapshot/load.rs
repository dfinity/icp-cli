use clap::Args;
use ic_management_canister_types::LoadCanisterSnapshotArgs;

use crate::commands::{
    args,
    canister::snapshot::{SnapshotId, ensure_canister_stopped},
};
use icp::context::Context;

#[derive(Debug, Args)]
pub struct LoadArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// The ID of the snapshot to load.
    snapshot: SnapshotId,
}

pub async fn exec(ctx: &Context, args: &LoadArgs) -> Result<(), anyhow::Error> {
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

    // Ensure canister is stopped
    let (status,) = mgmt.canister_status(&cid).await?;
    ensure_canister_stopped(status.status, &args.cmd_args.canister.to_string())?;

    // Load snapshot
    mgmt.load_canister_snapshot(
        &cid,
        &LoadCanisterSnapshotArgs {
            canister_id: cid,
            snapshot_id: args.snapshot.0.clone(),
            sender_canister_version: None,
        },
    )
    .await?;

    eprintln!(
        "Loaded snapshot {} into canister '{}'",
        args.snapshot, args.cmd_args.canister,
    );

    Ok(())
}
