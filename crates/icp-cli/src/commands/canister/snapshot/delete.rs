use clap::Args;
use ic_management_canister_types::DeleteCanisterSnapshotArgs;
use ic_utils::interfaces::ManagementCanister;
use icp::context::Context;

use super::SnapshotId;
use crate::commands::args;

/// Delete a canister snapshot
#[derive(Debug, Args)]
pub(crate) struct DeleteArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// The snapshot ID to delete (hex-encoded)
    snapshot_id: SnapshotId,
}

pub(crate) async fn exec(ctx: &Context, args: &DeleteArgs) -> Result<(), anyhow::Error> {
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

    let delete_args = DeleteCanisterSnapshotArgs {
        canister_id: cid,
        snapshot_id: args.snapshot_id.0.clone(),
    };

    mgmt.delete_canister_snapshot(&cid, &delete_args).await?;

    let name = &args.cmd_args.canister;
    ctx.term.write_line(&format!(
        "Deleted snapshot {id} from canister {name} ({cid})",
        id = hex::encode(&args.snapshot_id.0),
    ))?;

    Ok(())
}
