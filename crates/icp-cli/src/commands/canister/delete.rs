use anyhow::anyhow;
use candid::Principal;
use clap::Args;
use ic_management_canister_types::CanisterIdRecord;
use icp::context::{CanisterSelection, Context};

use crate::{
    commands::args,
    operations::{proxy_management, recover_cycles},
};

/// Delete a canister from a network
#[derive(Debug, Args)]
pub(crate) struct DeleteArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// Principal of a proxy canister to route the management canister call through.
    #[arg(long)]
    pub(crate) proxy: Option<Principal>,

    /// Skip recovering the canister's liquid cycles to your cycles-ledger
    /// account before deletion (they are burned instead).
    #[arg(long)]
    pub(crate) no_recover_cycles: bool,
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

    if !args.no_recover_cycles {
        let destination = agent
            .get_principal()
            .map_err(|e| anyhow!("could not determine caller principal: {e}"))?;
        recover_cycles::recover_cycles_before_delete(&agent, args.proxy, cid, destination).await?;
    }

    // delete_canister requires the canister be stopped; stopping an
    // already-stopped canister is a no-op, and the recovery step leaves it running.
    proxy_management::stop_canister(&agent, args.proxy, CanisterIdRecord { canister_id: cid })
        .await?;
    proxy_management::delete_canister(&agent, args.proxy, CanisterIdRecord { canister_id: cid })
        .await?;

    // Remove canister ID from the id store if it was referenced by name
    if let CanisterSelection::Named(canister_name) = &selections.canister {
        ctx.remove_canister_id_for_env(canister_name, &selections.environment)
            .await?;
        ctx.update_custom_domains(&selections.environment).await;
    }

    Ok(())
}
