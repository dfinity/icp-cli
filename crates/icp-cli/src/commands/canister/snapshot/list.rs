use clap::Args;
use ic_management_canister_types::Snapshot;
use indicatif::HumanBytes;
use time::{OffsetDateTime, macros::format_description};

use crate::commands::{args, canister::snapshot::SnapshotId};
use icp::context::Context;

#[derive(Debug, Args)]
pub struct ListArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
}

pub async fn exec(ctx: &Context, args: &ListArgs) -> Result<(), anyhow::Error> {
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

    let (snapshots,) = mgmt.list_canister_snapshots(&cid).await?;

    if snapshots.is_empty() {
        eprintln!(
            "No snapshots found for canister '{}'",
            args.cmd_args.canister
        );
    } else {
        for snapshot in snapshots {
            print_snapshot(&snapshot);
        }
    }
    Ok(())
}

fn print_snapshot(snapshot: &Snapshot) {
    let time_fmt = format_description!("[year]-[month]-[day] [hour]:[minute]:[second] UTC");

    eprintln!(
        "{}: {}, taken at {}",
        SnapshotId(snapshot.id.clone()),
        HumanBytes(snapshot.total_size),
        OffsetDateTime::from_unix_timestamp_nanos(snapshot.taken_at_timestamp as i128)
            .unwrap()
            .format(time_fmt)
            .unwrap()
    );
}
