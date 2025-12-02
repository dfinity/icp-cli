use clap::Args;
use ic_management_canister_types::TakeCanisterSnapshotArgs;

use crate::commands::{
    args,
    canister::snapshot::{SnapshotId, ensure_canister_stopped},
};
use icp::context::Context;

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// If a snapshot ID is specified, this snapshot will replace it and reuse the ID.
    #[arg(long)]
    replace: Option<SnapshotId>,
}

pub async fn exec(ctx: &Context, args: &CreateArgs) -> Result<(), anyhow::Error> {
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

    // Create snapshot
    let (snapshot,) = mgmt
        .take_canister_snapshot(
            &cid,
            &TakeCanisterSnapshotArgs {
                canister_id: cid,
                replace_snapshot: args.replace.as_ref().map(|id| id.0.clone()),
            },
        )
        .await?;

    eprintln!(
        "Created a new snapshot of canister '{}'. Snapshot ID: '{}'",
        args.cmd_args.canister,
        SnapshotId(snapshot.id)
    );

    Ok(())
}
