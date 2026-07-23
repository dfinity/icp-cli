use async_trait::async_trait;
use candid::Principal;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::Agent;
use icp::{Canister, canister::recipe::RemoteResourceResolve, canister::script, prelude::PathBuf};
use icp_deploy_canister::sync_exec::{
    ScriptInvocation, ScriptRunError, ScriptRunner, StepProgress,
};
use icp_deploy_canister::{SyncCanisterError, SyncStepContext, run_sync_steps};
use snafu::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tracing::error;

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
    error: SyncCanisterError,
    progress_output: Vec<String>,
}

/// [`ScriptRunner`] backed by the host subprocess executor. Sync-step dispatch
/// and the `ICP_CLI_*` environment are assembled by `icp_deploy_canister`; this
/// only spawns the resolved command(s).
struct HostScriptRunner;

#[async_trait]
impl ScriptRunner for HostScriptRunner {
    async fn run_script(
        &self,
        invocation: ScriptInvocation,
        stdio: Option<Sender<String>>,
    ) -> Result<Vec<String>, ScriptRunError> {
        let env_refs: Vec<(&str, &str)> = invocation
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        script::execute_commands(&invocation.commands, &invocation.cwd, &env_refs, stdio)
            .await
            .map_err(|source| ScriptRunError {
                source: Box::new(source),
            })?;
        // Persistent stderr is a sync-plugin feature only; script steps don't
        // currently retain any output past the rolling step view.
        Ok(vec![])
    }
}

/// [`StepProgress`] that frames each sync step on the canister's multi-step
/// progress bar and streams the step's output lines to it.
struct BarStepProgress<'a> {
    pb: &'a mut MultiStepProgressBar,
}

#[async_trait]
impl StepProgress for BarStepProgress<'_> {
    fn begin_step(&mut self, header: String) -> Option<Sender<String>> {
        Some(self.pb.begin_step(header))
    }

    async fn end_step(&mut self) {
        self.pb.end_step().await;
    }
}

/// Synchronize a single canister's steps through the library, framing progress
/// on `pb`. Environment variables are applied separately by the caller.
#[allow(clippy::too_many_arguments)]
async fn sync_canister(
    agent: &Agent,
    resolver: &dyn RemoteResourceResolve,
    canister_path: PathBuf,
    canister_id: Principal,
    canister_info: &Canister,
    environment: &str,
    network: &str,
    canister_ids: &BTreeMap<String, Principal>,
    proxy: Option<Principal>,
    pb: &mut MultiStepProgressBar,
) -> Result<Vec<String>, SyncCanisterError> {
    let ctx = SyncStepContext {
        canister_path,
        canister_id,
        environment: environment.to_owned(),
        network: network.to_owned(),
        canister_ids: canister_ids.clone(),
        proxy,
    };
    let mut progress = BarStepProgress { pb };
    run_sync_steps(
        canister_info,
        &ctx,
        agent,
        resolver,
        &HostScriptRunner,
        Some(&mut progress),
    )
    .await
}

/// Orchestrates syncing multiple canisters with progress tracking
#[allow(clippy::too_many_arguments)]
pub(crate) async fn sync_many(
    agent: Agent,
    resolver: Arc<dyn RemoteResourceResolve>,
    canisters: Vec<(Principal, PathBuf, Canister)>,
    environment: String,
    network: String,
    canister_ids: BTreeMap<String, Principal>,
    proxy: Option<Principal>,
    debug: bool,
) -> Result<(), SyncOperationError> {
    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for (cid, canister_path, canister_info) in canisters {
        let mut pb = progress_manager.create_multi_step_progress_bar(&canister_info.name, "Sync");

        let fut = {
            let agent = agent.clone();
            let resolver = resolver.clone();
            let environment = environment.clone();
            let network = network.clone();
            let canister_ids = canister_ids.clone();

            async move {
                let sync_result = sync_canister(
                    &agent,
                    resolver.as_ref(),
                    canister_path,
                    cid,
                    &canister_info,
                    &environment,
                    &network,
                    &canister_ids,
                    proxy,
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

                // Print stderr lines the plugin emitted; the rolling buffer
                // discards them on success, but they belong on the persistent
                // output channel.
                if let Ok(lines) = &result {
                    for line in lines {
                        eprintln!("[{}] {line}", canister_info.name);
                    }
                }

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
            error!(
                "----- Failed to sync canister '{}': {} -----",
                failure.canister_name, failure.canister_id,
            );
            error!("'{}'", failure.error);
            {
                use std::error::Error;
                let mut cause = failure.error.source();
                while let Some(err) = cause {
                    error!("  caused by: {err}");
                    cause = err.source();
                }
            }
            for line in &failure.progress_output {
                error!("{line}");
            }
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
