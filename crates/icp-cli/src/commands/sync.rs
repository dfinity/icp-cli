use anyhow::{anyhow, bail};
use candid::Principal;
use clap::Args;
use futures::future::try_join_all;
use ic_management_canister_types::{CanisterId, CanisterIdRecord, CanisterStatusType};
use icp::context::{CanisterSelection, Context, EnvironmentSelection};
use icp::identity::IdentitySelection;
use std::collections::BTreeMap;
use tracing::info;

use icp::Canister;

use crate::{
    operations::{binding_env_vars::set_binding_env_vars_many, proxy_management, sync::sync_many},
    options::{EnvironmentOpt, IdentityOpt},
};

/// Synchronize canisters
#[derive(Debug, Args)]
pub(crate) struct SyncArgs {
    /// Canister names (if empty, sync all canisters in environment)
    pub(crate) canisters: Vec<String>,

    /// Principal of a proxy canister to route sync plugin calls to the target canister through.
    #[arg(long)]
    pub(crate) proxy: Option<Principal>,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,
}

pub(crate) async fn exec(ctx: &Context, args: &SyncArgs) -> Result<(), anyhow::Error> {
    // Get environment and identity selections
    let environment_selection: EnvironmentSelection = args.environment.clone().into();
    let identity_selection: IdentitySelection = args.identity.clone().into();

    // Get environment
    let env = ctx.get_environment(&environment_selection).await?;

    // Determine which canisters to sync
    let cnames = match args.canisters.is_empty() {
        // No canisters specified - sync all in environment
        true => env.canisters.keys().cloned().collect(),

        // Individual canisters specified
        false => args.canisters.clone(),
    };

    // Skip doing any work if no canisters are targeted
    if cnames.is_empty() {
        return Ok(());
    }

    // Get agent
    let agent = ctx
        .get_agent_for_env(&identity_selection, &environment_selection)
        .await?;

    // Prepare list of canisters with their info for syncing
    let sync_canisters = try_join_all(cnames.iter().map(|name| async {
        let (canister_path, info) = ctx
            .get_canister_and_path_for_env(name, &environment_selection)
            .await?;
        let cid = ctx
            .get_canister_id_for_env(
                &CanisterSelection::Named(name.clone()),
                &environment_selection,
            )
            .await?;
        Ok::<_, anyhow::Error>((cid, canister_path.clone(), info.clone()))
    }))
    .await?;

    // Filter out canisters with no sync steps
    let sync_canisters: Vec<_> = sync_canisters
        .into_iter()
        .filter(|(_, _, info)| !info.sync.steps.is_empty())
        .collect();

    if sync_canisters.is_empty() {
        info!("No canisters have sync steps configured");
        return Ok(());
    }

    // icp sync is sync-only and does not manage lifecycle: unlike deploy it will
    // NOT start the canister for the user. Asset sync requires a Running canister,
    // so detect a non-Running canister up front and abort with an actionable error
    // instead of letting the plugin's first call fail with a cryptic IC0508
    // ("canister is stopped ... does not have a CallContextManager").
    let proxy = args.proxy;
    try_join_all(sync_canisters.iter().map(|(cid, _, _)| {
        let agent = agent.clone();
        let cid = *cid;
        async move {
            let status = proxy_management::canister_status(
                &agent,
                proxy,
                CanisterIdRecord {
                    canister_id: CanisterId::from(cid),
                },
            )
            .await
            .map_err(|e| anyhow!(e))?
            .status;
            if !matches!(status, CanisterStatusType::Running) {
                bail!(
                    "Canister {cid} is {status:?}; asset sync requires it to be Running. \
                     Start it with `icp canister start {cid}` and retry."
                );
            }
            Ok::<_, anyhow::Error>(())
        }
    }))
    .await?;

    info!("Syncing canisters:");

    let canister_ids: BTreeMap<String, Principal> = ctx
        .ids_by_environment(&environment_selection)
        .await?
        .into_iter()
        .collect();

    // Apply the generated `PUBLIC_CANISTER_ID:*` environment variables before
    // syncing. `deploy` does this, but standalone `icp sync` previously did not,
    // so a synced canister could run against stale/absent binding ids.
    let target_canisters: Vec<(Principal, Canister)> = sync_canisters
        .iter()
        .map(|(cid, _, info)| (*cid, info.clone()))
        .collect();
    set_binding_env_vars_many(
        agent.clone(),
        args.proxy,
        environment_selection.name(),
        target_canisters,
        canister_ids.clone(),
        ctx.debug,
    )
    .await?;

    let resolver = ctx.resource_resolver()?;
    sync_many(
        agent,
        resolver,
        sync_canisters,
        environment_selection.name().to_owned(),
        env.network.name.clone(),
        canister_ids,
        args.proxy,
        ctx.debug,
    )
    .await?;

    Ok(())
}
