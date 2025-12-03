use console::Term;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, export::Principal};
use icp::{
    Canister,
    canister::sync::{Params, Synchronize, SynchronizeError},
    prelude::PathBuf,
};
use snafu::prelude::*;
use std::sync::Arc;

use crate::progress::{MultiStepProgressBar, ProgressManager, ProgressManagerSettings};

#[derive(Debug, Snafu)]
#[snafu(display("one or more canisters failed to sync"))]
pub struct SyncOperationError;

/// Synchronizes a single canister using its configured sync steps
async fn sync_canister(
    syncer: &Arc<dyn Synchronize>,
    agent: &Agent,
    _term: &Term,
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
    term: Arc<Term>,
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

                // After progress bar is finished, dump the output if sync failed
                if let Err(e) = &result {
                    for line in pb.dump_output() {
                        let _ = term.write_line(&line);
                    }
                    let _ = term.write_line(&format!("Failed to sync canister: {e}"));
                    let _ = term.write_line("");
                }

                result
            }
        };

        futs.push_back(fut);
    }

    // Consume the set of futures and collect errors
    let mut found_error = false;
    while let Some(res) = futs.next().await {
        if res.is_err() {
            found_error = true;
        }
    }

    if found_error {
        return SyncOperationSnafu.fail();
    }

    Ok(())
}
