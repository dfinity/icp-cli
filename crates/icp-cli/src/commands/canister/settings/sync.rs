use anyhow::bail;
use candid::Principal;
use clap::Args;
use icp_app::context::Context;
use icp_project::host::CanisterSelection;
use tracing::warn;

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
        .host
        .get_canister_and_path_for_env(name, &selections.environment)
        .await?;

    let agent = ctx
        .get_agent(
            &selections.identity,
            &selections.network,
            &selections.environment,
        )
        .await?;

    let calls = icp_app::calls::calls(agent.clone(), args.proxy)?;
    let cid = ctx
        .get_canister_id(
            &selections.canister,
            &selections.network,
            &selections.environment,
        )
        .await?;
    let ids = ctx
        .host
        .ids_by_environment(&selections.environment)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;

    let unresolved =
        icp_project::operations::settings::sync_settings(calls.as_ref(), &cid, &canister, &ids)
            .await?;
    for controller_name in &unresolved {
        warn!(
            "Controller canister '{controller_name}' for '{name}' has not been created yet; \
             it will be set as a controller once created."
        );
    }
    Ok(())
}
