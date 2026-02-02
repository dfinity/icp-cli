use anyhow::bail;
use byte_unit::{Byte, UnitType};
use clap::Args;
use ic_management_canister_types::{CanisterStatusType, TakeCanisterSnapshotArgs};
use ic_utils::interfaces::ManagementCanister;
use icp::context::Context;

use super::SnapshotId;
use crate::{commands::args, operations::misc::format_timestamp};

#[derive(Debug, Args)]
pub(crate) struct CreateArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Replace an existing snapshot instead of creating a new one.
    /// The old snapshot will be deleted once the new one is successfully created.
    #[arg(long)]
    replace: Option<SnapshotId>,
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

    let mgmt = ManagementCanister::create(&agent);

    // Check canister status - must be stopped to create a snapshot
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

    let take_args = TakeCanisterSnapshotArgs {
        canister_id: cid,
        replace_snapshot: args.replace.as_ref().map(|s| s.0.clone()),
    };

    let (snapshot,) = mgmt.take_canister_snapshot(&cid, &take_args).await?;

    ctx.term.write_line(&format!(
        "Created snapshot {id} for canister {name} ({cid})",
        id = hex::encode(&snapshot.id),
    ))?;
    ctx.term.write_line(&format!(
        "  Timestamp: {}",
        format_timestamp(snapshot.taken_at_timestamp)
    ))?;
    ctx.term.write_line(&format!(
        "  Size: {}",
        Byte::from_u64(snapshot.total_size).get_appropriate_unit(UnitType::Binary)
    ))?;

    Ok(())
}
