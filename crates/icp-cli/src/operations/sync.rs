use async_trait::async_trait;
use candid::Principal;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::Agent;
use icp::{
    Canister,
    canister::sync::{Synchronize, SynchronizeError},
    package::PackageCache,
    prelude::PathBuf,
};
use icp_deploy_canister::manifest::adapter::prebuilt::SourceField;
use icp_deploy_canister::sync_exec::{
    PluginExecutor, PluginExecutorError, PluginInvocation, ScriptInvocation, StepProgress,
};
use icp_deploy_canister::{SyncCanisterError, SyncStepContext, run_sync_steps};
use snafu::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Mutex;
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

/// Per-canister mutable state guarded so the `&self` [`PluginExecutor`] can drive
/// the (mutable, sequential) progress bar.
struct SyncStepState<'a> {
    pb: &'a mut MultiStepProgressBar,
    /// 1-based index of the step about to run, for the progress header.
    next: usize,
}

/// [`PluginExecutor`] that runs a resolved step via the host [`Synchronize`]
/// implementation (WASI plugin / subprocess script) and frames it on the
/// canister's multi-step progress bar. The library owns the step loop and all
/// input derivation ([`run_sync_steps`]); this only performs the host action and
/// streams its output.
struct AgentSyncExecutor<'a> {
    syncer: Arc<dyn Synchronize>,
    agent: Agent,
    pkg_cache: &'a PackageCache,
    total: usize,
    state: Mutex<SyncStepState<'a>>,
}

impl AgentSyncExecutor<'_> {
    /// Frame a step on the shared progress bar: advance the counter, print the
    /// header, run `f` against a fresh line sender, and close the step. Holding
    /// the guard across `f` keeps steps framed sequentially on the shared bar.
    async fn framed<F, Fut>(
        &self,
        header: impl FnOnce(usize, usize) -> String,
        f: F,
    ) -> Result<Vec<String>, SynchronizeError>
    where
        F: FnOnce(tokio::sync::mpsc::Sender<String>) -> Fut,
        Fut: Future<Output = Result<Vec<String>, SynchronizeError>>,
    {
        let mut st = self.state.lock().await;
        st.next += 1;
        let header = header(st.next, self.total);
        let tx = st.pb.begin_step(header);
        let result = f(tx).await;
        st.pb.end_step().await;
        result
    }
}

#[async_trait]
impl PluginExecutor for AgentSyncExecutor<'_> {
    async fn run_plugin(
        &self,
        invocation: PluginInvocation,
        _progress: Option<&dyn StepProgress>,
    ) -> Result<Vec<String>, PluginExecutorError> {
        let src = match &invocation.source {
            SourceField::Local(l) => format!("path: {}", l.path),
            SourceField::Remote(r) => format!("url: {}", r.url),
        };
        self.framed(
            |n, total| format!("\nSyncing: plugin {src} {n} of {total}"),
            |tx| async move {
                self.syncer
                    .run_plugin(&invocation, &self.agent, Some(tx), self.pkg_cache)
                    .await
            },
        )
        .await
        .map_err(|source| PluginExecutorError::Plugin {
            source: Box::new(source),
        })
    }

    async fn run_script(
        &self,
        invocation: ScriptInvocation,
        _progress: Option<&dyn StepProgress>,
    ) -> Result<Vec<String>, PluginExecutorError> {
        let desc = invocation.commands.join("\n");
        self.framed(
            |n, total| format!("\nSyncing: script {desc} {n} of {total}"),
            |tx| async move { self.syncer.run_script(&invocation, Some(tx)).await },
        )
        .await
        .map_err(|source| PluginExecutorError::Script {
            source: Box::new(source),
        })
    }
}

/// Synchronize a single canister's steps through the library, framing progress
/// on `pb`. Environment variables are applied separately by the caller.
#[allow(clippy::too_many_arguments)]
async fn sync_canister(
    syncer: Arc<dyn Synchronize>,
    agent: Agent,
    canister_path: PathBuf,
    canister_id: Principal,
    canister_info: &Canister,
    environment: &str,
    network: &str,
    canister_ids: &BTreeMap<String, Principal>,
    proxy: Option<Principal>,
    pb: &mut MultiStepProgressBar,
    pkg_cache: &PackageCache,
) -> Result<Vec<String>, SyncCanisterError> {
    let ctx = SyncStepContext {
        canister_path,
        canister_id,
        environment: environment.to_owned(),
        network: network.to_owned(),
        canister_ids: canister_ids.clone(),
        proxy,
    };
    let executor = AgentSyncExecutor {
        syncer,
        agent,
        pkg_cache,
        total: canister_info.sync.steps.len(),
        state: Mutex::new(SyncStepState { pb, next: 0 }),
    };
    run_sync_steps(canister_info, &ctx, &executor, None).await
}

/// Orchestrates syncing multiple canisters with progress tracking
#[allow(clippy::too_many_arguments)]
pub(crate) async fn sync_many(
    syncer: Arc<dyn Synchronize>,
    agent: Agent,
    canisters: Vec<(Principal, PathBuf, Canister)>,
    environment: String,
    network: String,
    canister_ids: BTreeMap<String, Principal>,
    proxy: Option<Principal>,
    debug: bool,
    pkg_cache: &PackageCache,
) -> Result<(), SyncOperationError> {
    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for (cid, canister_path, canister_info) in canisters {
        let mut pb = progress_manager.create_multi_step_progress_bar(&canister_info.name, "Sync");

        let fut = {
            let agent = agent.clone();
            let syncer = syncer.clone();
            let environment = environment.clone();
            let network = network.clone();
            let canister_ids = canister_ids.clone();

            async move {
                let sync_result = sync_canister(
                    syncer,
                    agent,
                    canister_path,
                    cid,
                    &canister_info,
                    &environment,
                    &network,
                    &canister_ids,
                    proxy,
                    &mut pb,
                    pkg_cache,
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
