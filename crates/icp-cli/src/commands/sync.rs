use anyhow::anyhow;
use clap::Args;
use futures::future::try_join_all;
use icp::context::{Context, EnvironmentSelection};
use icp::identity::IdentitySelection;
use std::sync::Arc;

use crate::{
    operations::sync::{SyncOperationError, sync_many},
    options::{EnvironmentOpt, IdentityOpt},
};

#[derive(Debug, Args)]
pub(crate) struct SyncArgs {
    /// Canister names (if empty, sync all canisters in environment)
    pub(crate) canisters: Vec<String>,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,

    #[command(flatten)]
    pub(crate) identity: IdentityOpt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    SyncOperation(#[from] SyncOperationError),

    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub(crate) async fn exec(ctx: &Context, args: &SyncArgs) -> Result<(), CommandError> {
    // Get environment and identity selections
    let environment_selection: EnvironmentSelection = args.environment.clone().into();
    let identity_selection: IdentitySelection = args.identity.clone().into();

    // Get environment
    let env = ctx
        .get_environment(&environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;

    // Determine which canisters to sync
    let cnames = match args.canisters.is_empty() {
        // No canisters specified - sync all in environment
        true => env.canisters.keys().cloned().collect(),

        // Individual canisters specified
        false => args.canisters.clone(),
    };

    // Validate all specified canisters exist in project and environment
    for name in &cnames {
        ctx.assert_env_contains_canister(name, &environment_selection)
            .await
            .map_err(|e| anyhow!(e))?;
    }

    // Skip doing any work if no canisters are targeted
    if cnames.is_empty() {
        return Ok(());
    }

    // Get agent
    let agent = ctx
        .get_agent_for_env(&identity_selection, &environment_selection)
        .await
        .map_err(|e| anyhow!(e))?;

    // Prepare list of canisters with their info for syncing
    let env_canisters = &env.canisters;
    let sync_canisters = try_join_all(cnames.iter().map(|name| {
        let environment_selection = environment_selection.clone();
        async move {
            let cid = ctx
                .get_canister_id_for_env(name, &environment_selection)
                .await
                .map_err(|e| anyhow!(e))?;
            let (canister_path, info) = env_canisters
                .get(name)
                .ok_or_else(|| anyhow!("Canister id exists but no canister info"))?;
            Ok::<_, anyhow::Error>((cid, canister_path.clone(), info.clone()))
        }
    }))
    .await?;

    // Filter out canisters with no sync steps
    let sync_canisters: Vec<_> = sync_canisters
        .into_iter()
        .filter(|(_, _, info)| !info.sync.steps.is_empty())
        .collect();

    if sync_canisters.is_empty() {
        let _ = ctx
            .term
            .write_line("No canisters have sync steps configured");
        return Ok(());
    }

    let _ = ctx.term.write_line("Syncing canisters:");

    sync_many(
        ctx.syncer.clone(),
        agent,
        Arc::new(ctx.term.clone()),
        sync_canisters,
        ctx.debug,
    )
    .await?;

    let _ = ctx.term.write_line("\nCanisters synced successfully");

    Ok(())
}
