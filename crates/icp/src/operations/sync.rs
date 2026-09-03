use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use candid::Principal;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::Agent;
use icp_deploy_canister::manifest::adapter::prebuilt::SourceField;
use icp_deploy_canister::sync_exec::{
    PluginExecutor, PluginExecutorError, PluginInvocation, ScriptInvocation, ScriptRunError,
    ScriptRunner, StepProgress,
};
use icp_deploy_canister::{SyncCanisterError, SyncStepContext, run_sync_steps};
use icp_events::{StepOutcome, StepReporter, TaskOutcome};
use snafu::prelude::*;

use crate::{
    Canister,
    canister::recipe::RemoteResourceResolve,
    canister::sync::{Synchronize, SynchronizeError},
    operations::task::{Reporter, Task, TaskReporter},
    prelude::{Path, PathBuf},
};

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to sync."))]
pub struct SyncOperationError {
    names: Vec<String>,
}

/// Sync-step executor that runs a resolved step through the host
/// [`Synchronize`] implementation (WASI plugin / subprocess script) and reports
/// it as one step of the canister's sync task. The library owns the step loop
/// and all input derivation ([`run_sync_steps`]); this only performs the host
/// action and reports what it produces.
struct AgentSyncExecutor<'a> {
    syncer: Arc<dyn Synchronize>,
    agent: Agent,
    resolver: Arc<dyn RemoteResourceResolve>,
    task: &'a TaskReporter,
    total: usize,
    /// How many steps have been started. The library runs the steps in order and
    /// waits for each, so this counts up to the 1-based index of the step about
    /// to run.
    started: AtomicUsize,
}

impl AgentSyncExecutor<'_> {
    /// Report one step around the host action: open it under `label`, run `f`
    /// with the step's reporter, and close it with the outcome.
    async fn stepped<F, Fut>(&self, label: String, f: F) -> Result<Vec<String>, SynchronizeError>
    where
        F: FnOnce(StepReporter) -> Fut,
        Fut: Future<Output = Result<Vec<String>, SynchronizeError>>,
    {
        let number = self.started.fetch_add(1, Ordering::Relaxed) + 1;
        let reporter = self.task.step(number, self.total, label);
        let result = f(reporter.clone()).await;
        reporter.done(match &result {
            Ok(_) => StepOutcome::Succeeded,
            Err(_) => StepOutcome::Failed,
        });
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
        self.stepped(format!("plugin {src}"), |reporter| async move {
            self.syncer
                .run_plugin(&invocation, &self.agent, &reporter, self.resolver.as_ref())
                .await
        })
        .await
        .map_err(|source| PluginExecutorError {
            source: Box::new(source),
        })
    }
}

#[async_trait]
impl ScriptRunner for AgentSyncExecutor<'_> {
    async fn run_script(
        &self,
        invocation: ScriptInvocation,
        _progress: Option<&dyn StepProgress>,
    ) -> Result<Vec<String>, ScriptRunError> {
        let label = format!("script {}", invocation.commands.join("\n"));
        self.stepped(label, |reporter| async move {
            self.syncer.run_script(&invocation, &reporter).await
        })
        .await
        .map_err(|source| ScriptRunError {
            source: Box::new(source),
        })
    }
}

/// Synchronize a single canister's steps through the library, reporting each as
/// a step of `task` and returning the stderr lines the steps retained for the
/// persistent output channel. Environment variables are applied separately by
/// the caller.
#[allow(clippy::too_many_arguments)]
async fn sync_canister(
    syncer: Arc<dyn Synchronize>,
    resolver: Arc<dyn RemoteResourceResolve>,
    agent: Agent,
    canister_path: PathBuf,
    project_dir: &Path,
    canister_id: Principal,
    canister_info: &Canister,
    environment: &str,
    network: &str,
    canister_ids: &BTreeMap<String, Principal>,
    proxy: Option<Principal>,
    task: &TaskReporter,
) -> Result<Vec<String>, SyncCanisterError> {
    let ctx = SyncStepContext {
        canister_path,
        project_dir: project_dir.to_path_buf(),
        canister_id,
        canister_name: canister_info.name.clone(),
        environment: environment.to_owned(),
        network: network.to_owned(),
        canister_ids: canister_ids.clone(),
        proxy,
    };
    let executor = AgentSyncExecutor {
        syncer,
        agent,
        resolver,
        task,
        total: canister_info.sync.steps.len(),
        started: AtomicUsize::new(0),
    };
    run_sync_steps(canister_info, &ctx, &executor, &executor, None).await
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
#[allow(clippy::too_many_arguments)]
pub async fn sync_many(
    syncer: Arc<dyn Synchronize>,
    resolver: Arc<dyn RemoteResourceResolve>,
    agent: Agent,
    canisters: Vec<(Principal, PathBuf, Canister)>,
    project_dir: PathBuf,
    environment: String,
    network: String,
    canister_ids: BTreeMap<String, Principal>,
    proxy: Option<Principal>,
    reporter: &Reporter,
) -> Result<(), SyncOperationError> {
    let mut futs = FuturesOrdered::new();

    for (cid, canister_path, canister_info) in canisters {
        let task = reporter.task(Task::sync(canister_info.name.clone(), cid));

        let fut = {
            let agent = agent.clone();
            let syncer = syncer.clone();
            let resolver = resolver.clone();
            let environment = environment.clone();
            let network = network.clone();
            let canister_ids = canister_ids.clone();
            let project_dir = project_dir.clone();

            async move {
                let result = sync_canister(
                    syncer,
                    resolver,
                    agent,
                    canister_path,
                    &project_dir,
                    cid,
                    &canister_info,
                    &environment,
                    &network,
                    &canister_ids,
                    proxy,
                    &task,
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
