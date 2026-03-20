use std::io::stdout;

use byte_unit::{Byte, UnitType};
use clap::Args;
use ic_utils::interfaces::ManagementCanister;
use icp::context::Context;
use itertools::Itertools;
use serde::Serialize;

use crate::{commands::args, operations::misc::format_timestamp};

/// List all snapshots for a canister
#[derive(Debug, Args)]
pub(crate) struct ListArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Output command results as JSON
    #[arg(long, conflicts_with = "quiet")]
    pub(crate) json: bool,

    /// Suppress human-readable output; print only snapshot IDs
    #[arg(long, short)]
    pub(crate) quiet: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &ListArgs) -> Result<(), anyhow::Error> {
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

    let (snapshots,) = mgmt.list_canister_snapshots(&cid).await?;

    let name = &args.cmd_args.canister;
    if args.json {
        serde_json::to_writer(
            stdout(),
            &JsonSnapshotList {
                snapshots: snapshots
                    .into_iter()
                    .map(|snapshot| JsonSnapshotListEntry {
                        snapshot_id: hex::encode(snapshot.id),
                        taken_at_timestamp: snapshot.taken_at_timestamp,
                        total_size_bytes: snapshot.total_size,
                    })
                    .collect(),
            },
        )?;
        return Ok(());
    } else if args.quiet {
        println!(
            "{}",
            snapshots.iter().map(|s| hex::encode(&s.id)).format("\n")
        );
    }
    if snapshots.is_empty() {
        println!("No snapshots found for canister {name} ({cid})");
    } else {
        println!("Snapshots for canister {name} ({cid}):");
        for snapshot in snapshots {
            println!(
                "  {id}: {size}, taken at {timestamp}",
                id = hex::encode(&snapshot.id),
                size = Byte::from_u64(snapshot.total_size).get_appropriate_unit(UnitType::Binary),
                timestamp = format_timestamp(snapshot.taken_at_timestamp),
            );
        }
    }

    Ok(())
}

#[derive(Serialize)]
struct JsonSnapshotList {
    snapshots: Vec<JsonSnapshotListEntry>,
}

#[derive(Serialize)]
struct JsonSnapshotListEntry {
    snapshot_id: String,
    taken_at_timestamp: u64,
    total_size_bytes: u64,
}
