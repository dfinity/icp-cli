use anyhow::bail;
use clap::Args;
use ic_management_canister_types::{CanisterStatusType, LoadCanisterSnapshotArgs};
use ic_utils::interfaces::ManagementCanister;
use icp::context::Context;

use super::SnapshotId;
use crate::commands::args;

/// Restore a canister from a snapshot
#[derive(Debug, Args)]
pub(crate) struct RestoreArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// The snapshot ID to restore (hex-encoded)
    snapshot_id: SnapshotId,
}

pub(crate) async fn exec(ctx: &Context, args: &RestoreArgs) -> Result<(), anyhow::Error> {
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

    let mgmt = ManagementCanister::create(&agent);

    // Check canister status - must be stopped to restore a snapshot
    let name = &args.cmd_args.canister;
    let (status,) = mgmt.canister_status(&cid).await?;
    match status.status {
        CanisterStatusType::Running => {
            bail!(
                "Canister {name} ({cid}) is currently running. Please stop the canister first with `icp canister stop`."
            );
        }
        CanisterStatusType::Stopping => {
            bail!("Canister {name} ({cid}) is still stopping. Please wait for it to fully stop.");
        }
        CanisterStatusType::Stopped => {}
    }

    let load_args = LoadCanisterSnapshotArgs {
        canister_id: cid,
        snapshot_id: args.snapshot_id.0.clone(),
        sender_canister_version: None,
    };

    mgmt.load_canister_snapshot(&cid, &load_args).await?;

    ctx.term.write_line(&format!(
        "Restored canister {name} ({cid}) from snapshot {id}",
        id = hex::encode(&args.snapshot_id.0),
    ))?;

    Ok(())
}
