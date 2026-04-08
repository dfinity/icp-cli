use anyhow::bail;
use candid::Principal;
use clap::Args;
use ic_management_canister_types::{
    CanisterIdRecord, CanisterStatusType, LoadCanisterSnapshotArgs,
};
use icp::context::Context;
use tracing::info;

use super::SnapshotId;
use crate::{commands::args, operations::proxy_management};

/// Restore a canister from a snapshot
#[derive(Debug, Args)]
pub(crate) struct RestoreArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// The snapshot ID to restore (hex-encoded)
    snapshot_id: SnapshotId,

    /// Principal of a proxy canister to route the management canister calls through.
    #[arg(long)]
    proxy: Option<Principal>,
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

    // Check canister status - must be stopped to restore a snapshot
    let name = &args.cmd_args.canister;
    let status = proxy_management::canister_status(
        &agent,
        args.proxy,
        CanisterIdRecord { canister_id: cid },
    )
    .await?;
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

    proxy_management::load_canister_snapshot(&agent, args.proxy, load_args).await?;

    info!(
        "Restored canister {name} ({cid}) from snapshot {id}",
        id = hex::encode(&args.snapshot_id.0),
    );

    Ok(())
}
