use byte_unit::{Byte, UnitType};
use clap::Args;
use ic_utils::interfaces::ManagementCanister;
use icp::context::Context;

use crate::{commands::args, operations::misc::format_timestamp};

#[derive(Debug, Args)]
pub(crate) struct ListArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
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
    if snapshots.is_empty() {
        ctx.term
            .write_line(&format!("No snapshots found for canister {name} ({cid})"))?;
    } else {
        ctx.term
            .write_line(&format!("Snapshots for canister {name} ({cid}):"))?;
        for snapshot in snapshots {
            ctx.term.write_line(&format!(
                "  {id}: {size}, taken at {timestamp}",
                id = hex::encode(&snapshot.id),
                size = Byte::from_u64(snapshot.total_size).get_appropriate_unit(UnitType::Binary),
                timestamp = format_timestamp(snapshot.taken_at_timestamp),
            ))?;
        }
    }

    Ok(())
}
