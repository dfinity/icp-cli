use anyhow::anyhow;
use clap::Args;
use icp::context::{CanisterSelection, Context};

use crate::{
    commands::args,
    operations::sync::{SyncOperationError, sync_canister},
};

#[derive(Debug, Args)]
pub(crate) struct SyncArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    SyncOperation(#[from] SyncOperationError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub(crate) async fn exec(ctx: &Context, args: &SyncArgs) -> Result<(), CommandError> {
    let selections = args.cmd_args.selections();
    
    // Extract canister name (reject Principal)
    let canister_name = match selections.canister {
        CanisterSelection::Named(name) => name,
        CanisterSelection::Principal(_) => {
            return Err(anyhow!("Cannot sync canister by principal. Please specify a canister name"))?
        }
    };

    // Load the project to get canister info
    let _p = ctx.project.load().await.map_err(|e| anyhow!(e))?;
    
    // Get the environment
    let env = ctx
        .get_environment(&selections.environment)
        .await
        .map_err(|e| anyhow!(e))?;
    
    // Get canister info from environment (includes path and sync config)
    let (canister_path, canister_info) = env
        .canisters
        .get(&canister_name)
        .ok_or_else(|| anyhow!("Canister '{}' not found in environment '{}'", canister_name, env.name))?;

    // Check if canister has sync steps
    if canister_info.sync.steps.is_empty() {
        let _ = ctx.term.write_line(&format!(
            "Canister '{}' has no sync steps configured. Skipping.",
            canister_name
        ));
        return Ok(());
    }

    // Get canister ID
    let canister_id = ctx
        .get_canister_id_for_env(&canister_name, &selections.environment)
        .await
        .map_err(|e| anyhow!(e))?;

    // Get agent
    let agent = ctx
        .get_agent_for_env(&selections.identity, &selections.environment)
        .await
        .map_err(|e| anyhow!(e))?;

    // Create a single-step progress bar (or use multi-step if needed)
    // For single canister command, we'll use a simplified approach without progress bar
    // since it's synchronous and direct
    let mut pb = crate::progress::ProgressManager::new(crate::progress::ProgressManagerSettings {
        hidden: ctx.debug,
    })
    .create_multi_step_progress_bar(&canister_name, "Sync");

    // Execute sync (convert PathBuf types)
    let sync_result = sync_canister(
        &ctx.syncer,
        &agent,
        &ctx.term,
        canister_path.to_owned(),
        canister_id,
        canister_info,
        &mut pb,
    )
    .await;

    // Execute with progress tracking for final state
    let result = crate::progress::ProgressManager::execute_with_progress(
        &pb,
        async { sync_result },
        || format!("Synced successfully: {}", canister_id),
        |err| format!("Failed to sync canister: {}", err),
    )
    .await;

    // After progress bar is finished, dump the output if sync failed
    if let Err(ref e) = result {
        for line in pb.dump_output() {
            let _ = ctx.term.write_line(&line);
        }
        let _ = ctx
            .term
            .write_line(&format!("Failed to sync canister: {}", e));
        let _ = ctx.term.write_line("");
    }
    
    result?;

    let _ = ctx
        .term
        .write_line(&format!("Canister {} synced successfully", canister_name));

    Ok(())
}

