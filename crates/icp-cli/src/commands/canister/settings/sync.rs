use anyhow::bail;
use candid::Principal;
use clap::Args;
use icp::context::{CanisterSelection, Context};

use crate::commands::args::CanisterCommandArgs;

/// Synchronize a canister's settings with those defined in the project
#[derive(Debug, Args)]
pub(crate) struct SyncArgs {
    #[command(flatten)]
    cmd_args: CanisterCommandArgs,

    /// Principal of a proxy canister to route the management canister calls through.
    #[arg(long)]
    proxy: Option<Principal>,
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

    crate::operations::settings::sync_settings(&agent, args.proxy, &cid, &canister).await?;
    Ok(())
}
