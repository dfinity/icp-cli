use anyhow::bail;
use clap::Args;
use ic_utils::interfaces::ManagementCanister;
use icp::context::{CanisterSelection, Context};

use crate::commands::args::CanisterCommandArgs;

/// Synchronize a canister's settings with those defined in the project
#[derive(Debug, Args)]
pub(crate) struct SyncArgs {
    #[command(flatten)]
    cmd_args: CanisterCommandArgs,
}

pub(crate) async fn exec(ctx: &Context, args: &SyncArgs) -> Result<(), anyhow::Error> {
    let selections = args.cmd_args.selections();
    let CanisterSelection::Named(name) = &selections.canister else {
        bail!("canister name must be used for settings sync");
    };

    let (_, canister) = ctx
        .get_canister_and_path_for_env(name, &selections.environment)
        .await?;

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

    crate::operations::settings::sync_settings(&mgmt, &cid, &canister).await?;
    Ok(())
}
