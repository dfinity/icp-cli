use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, export::Principal};
use icp::{
    Canister,
    canister::sync::{Params, Synchronize, SynchronizeError},
    context::TermWriter,
    prelude::PathBuf,
};
use snafu::prelude::*;
use std::sync::Arc;

use crate::progress::{MultiStepProgressBar, ProgressManager, ProgressManagerSettings};

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to sync."))]
pub struct SyncOperationError {
    names: Vec<String>,
}

/// Holds error information from a failed canister sync operation
struct SyncFailure {
    canister_name: String,
    canister_id: Principal,
    error: SynchronizeError,
    progress_output: Vec<String>,
}

/// Synchronizes a single canister using its configured sync steps
async fn sync_canister(
    syncer: &Arc<dyn Synchronize>,
    agent: &Agent,
    _term: &TermWriter,
    canister_path: PathBuf,
    canister_id: Principal,
    canister_info: &Canister,
    pb: &mut MultiStepProgressBar,
) -> Result<(), SynchronizeError> {
    let step_count = canister_info.sync.steps.len();

    for (i, step) in canister_info.sync.steps.iter().enumerate() {
        // Indicate to user the current step being executed
        let current_step = i + 1;
        let pb_hdr = format!("\nSyncing: {step} {current_step} of {step_count}");

        let tx = pb.begin_step(pb_hdr);

        // Execute step
        let sync_result = syncer
            .sync(
                step,
                &Params {
                    path: canister_path.clone(),
                    cid: canister_id,
                },
                agent,
                Some(tx),
            )
            .await;

        // Ensure background receiver drains all messages
        pb.end_step().await;

        sync_result?;
    }

    Ok(())
}

/// Orchestrates syncing multiple canisters with progress tracking
pub(crate) async fn sync_many(
    syncer: Arc<dyn Synchronize>,
    agent: Agent,
    term: Arc<TermWriter>,
    canisters: Vec<(Principal, PathBuf, Canister)>,
    debug: bool,
) -> Result<(), SyncOperationError> {
    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for (cid, canister_path, canister_info) in canisters {
        let mut pb = progress_manager.create_multi_step_progress_bar(&canister_info.name, "Sync");

        let fut = {
            let agent = agent.clone();
            let syncer = syncer.clone();
            let term = term.clone();

            async move {
                // Define the sync logic
                let sync_result = sync_canister(
                    &syncer,
                    &agent,
                    &term,
                    canister_path,
                    cid,
                    &canister_info,
                    &mut pb,
                )
                .await;

                // Execute with progress tracking for final state
                let result = ProgressManager::execute_with_progress(
                    &pb,
                    async { sync_result },
                    || format!("Synced successfully: {cid}"),
                    |err| format!("Failed to sync canister: {err}"),
                )
                .await;

                // Map error to include canister context for deferred printing
                result.map_err(|error| SyncFailure {
                    canister_name: canister_info.name.clone(),
                    canister_id: cid,
                    error,
                    progress_output: pb.dump_output(debug),
                })
            }
        };

        futs.push_back(fut);
    }

    // Consume the set of futures and collect errors
    let mut errors: Vec<SyncFailure> = Vec::new();
    while let Some(res) = futs.next().await {
        if let Err(failure) = res {
            errors.push(failure);
        }
    }

    if !errors.is_empty() {
        // Print all errors in batch
        for failure in &errors {
            // Print progress output
            let _ = term.write_line("");
            let _ = term.write_line("");
            let _ = term.write_line(&format!(
                " ----- Failed to sync canister '{}': {} -----",
                failure.canister_name, failure.canister_id,
            ));
            let _ = term.write_line(&format!("Error: '{}'", failure.error));
            for line in &failure.progress_output {
                let _ = term.write_line(line);
            }

            let _ = term.write_line("");
        }

        return SyncOperationSnafu {
            names: errors
                .iter()
                .map(|e| e.canister_name.clone())
                .collect::<Vec<String>>(),
        }
        .fail();
    }

    Ok(())
}
