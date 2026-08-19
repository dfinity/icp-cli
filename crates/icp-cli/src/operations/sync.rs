use candid::Principal;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::Agent;
use icp::{
    Canister,
    canister::sync::{Params, Synchronize, SynchronizeError},
    package::PackageCache,
    prelude::PathBuf,
};
use icp_events::{Reporter, StepOutcome, TaskKind, TaskOutcome, TaskReporter};
use snafu::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to sync."))]
pub struct SyncOperationError {
    names: Vec<String>,
}

/// Synchronizes a single canister using its configured sync steps, returning
/// the stderr lines the steps retained for the persistent output channel.
async fn sync_canister(
    syncer: &Arc<dyn Synchronize>,
    agent: &Agent,
    canister_path: PathBuf,
    canister_id: Principal,
    canister_info: &Canister,
    environment: &str,
    network: &str,
    canister_ids: &BTreeMap<String, Principal>,
    proxy: Option<Principal>,
    task: &TaskReporter,
    pkg_cache: &PackageCache,
) -> Result<Vec<String>, SynchronizeError> {
    let step_count = canister_info.sync.steps.len();
    let mut stderr_lines = Vec::new();

    for (i, step) in canister_info.sync.steps.iter().enumerate() {
        let reporter = task.step(i + 1, step_count, step.to_string());

        let sync_result = syncer
            .sync(
                step,
                &Params {
                    path: canister_path.clone(),
                    cid: canister_id,
                    environment: environment.to_owned(),
                    network: network.to_owned(),
                    canister_ids: canister_ids.clone(),
                    proxy,
                },
                agent,
                &reporter,
                pkg_cache,
            )
            .await;

        reporter.done(match &sync_result {
            Ok(_) => StepOutcome::Succeeded,
            Err(_) => StepOutcome::Failed,
        });

        stderr_lines.extend(sync_result?);
    }

    Ok(stderr_lines)
}

/// The rendered `source()` chain of an error, outermost cause first.
fn error_causes(error: &dyn std::error::Error) -> Vec<String> {
    let mut causes = Vec::new();
    let mut cause = error.source();
    while let Some(err) = cause {
        causes.push(err.to_string());
        cause = err.source();
    }
    causes
}

/// Orchestrates syncing multiple canisters concurrently.
pub(crate) async fn sync_many(
    syncer: Arc<dyn Synchronize>,
    agent: Agent,
    canisters: Vec<(Principal, PathBuf, Canister)>,
    environment: String,
    network: String,
    canister_ids: BTreeMap<String, Principal>,
    proxy: Option<Principal>,
    pkg_cache: &PackageCache,
    reporter: &Reporter,
) -> Result<(), SyncOperationError> {
    let mut futs = FuturesOrdered::new();

    for (cid, canister_path, canister_info) in canisters {
        let task = reporter.task(TaskKind::Sync {
            canister: canister_info.name.clone(),
            canister_id: cid,
        });

        let fut = {
            let agent = agent.clone();
            let syncer = syncer.clone();
            let environment = environment.clone();
            let network = network.clone();
            let canister_ids = canister_ids.clone();

            async move {
                let result = sync_canister(
                    &syncer,
                    &agent,
                    canister_path,
                    cid,
                    &canister_info,
                    &environment,
                    &network,
                    &canister_ids,
                    proxy,
                    &task,
                    pkg_cache,
                )
                .await;

                match &result {
                    // The retained stderr lines ride on the outcome: the
                    // rolling step view discards them on success, but they
                    // belong on the persistent output channel.
                    Ok(stderr_lines) => task.finish(TaskOutcome::Succeeded {
                        retained_output: stderr_lines.clone(),
                    }),
                    Err(error) => task.finish(TaskOutcome::Failed {
                        message: error.to_string(),
                        causes: error_causes(error),
                    }),
                }

                result.map(|_| ()).map_err(|_| canister_info.name.clone())
            }
        };

        futs.push_back(fut);
    }

    // Consume the set of futures and collect the failed canister names; the
    // renderer owns displaying each failure's captured output.
    let mut failed: Vec<String> = Vec::new();
    while let Some(res) = futs.next().await {
        if let Err(name) = res {
            failed.push(name);
        }
    }

    if !failed.is_empty() {
        return SyncOperationSnafu { names: failed }.fail();
    }

    Ok(())
}
