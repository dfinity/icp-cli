use std::io::stdout;

use anyhow::bail;
use byte_unit::{Byte, UnitType};
use clap::Args;
use ic_management_canister_types::{
    CanisterIdRecord, CanisterStatusType, TakeCanisterSnapshotArgs,
};
use icp::context::Context;
use serde::Serialize;

use super::SnapshotId;
use crate::{commands::args, operations::misc::format_timestamp, operations::proxy_management};

/// Create a snapshot of a canister's state
#[derive(Debug, Args)]
pub(crate) struct CreateArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Replace an existing snapshot instead of creating a new one.
    /// The old snapshot will be deleted once the new one is successfully created.
    #[arg(long)]
    replace: Option<SnapshotId>,

    /// Output command results as JSON
    #[arg(long, conflicts_with = "quiet")]
    json: bool,

    /// Suppress human-readable output; print only snapshot ID
    #[arg(long, short)]
    quiet: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &CreateArgs) -> Result<(), anyhow::Error> {
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

    // Check canister status - must be stopped to create a snapshot
    let name = &args.cmd_args.canister;
    let status =
        proxy_management::canister_status(&agent, None, CanisterIdRecord { canister_id: cid })
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

    let take_args = TakeCanisterSnapshotArgs {
        canister_id: cid,
        replace_snapshot: args.replace.as_ref().map(|s| s.0.clone()),
        uninstall_code: None,
        sender_canister_version: None,
    };

    let snapshot = proxy_management::take_canister_snapshot(&agent, None, take_args).await?;
    if args.json {
        serde_json::to_writer(
            stdout(),
            &JsonSnapshotCreate {
                snapshot_id: hex::encode(&snapshot.id),
                taken_at_timestamp: snapshot.taken_at_timestamp,
                total_size_bytes: snapshot.total_size,
            },
        )?;
    } else if args.quiet {
        println!("{}", hex::encode(&snapshot.id));
    } else {
        println!(
            "Created snapshot {id} for canister {name} ({cid})",
            id = hex::encode(&snapshot.id),
        );
        println!(
            "  Timestamp: {}",
            format_timestamp(snapshot.taken_at_timestamp)
        );
        println!(
            "  Size: {}",
            Byte::from_u64(snapshot.total_size).get_appropriate_unit(UnitType::Binary)
        );
    }

    Ok(())
}

#[derive(Serialize)]
struct JsonSnapshotCreate {
    snapshot_id: String,
    taken_at_timestamp: u64,
    total_size_bytes: u64,
}
