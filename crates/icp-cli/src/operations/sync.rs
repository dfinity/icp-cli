use candid::Principal;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::Agent;
use icp::{
    Canister,
    canister::sync::{Params, Synchronize, SynchronizeError},
    package::PackageCache,
    prelude::PathBuf,
};
use icp_events::{Reporter, Task, TaskKind};
use snafu::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::error;

use crate::operations::step_replay::replay;

/// What a sync task's output is called when it is replayed after a failure.
const OUTPUT_LABEL: &str = "Sync";

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
    step_output: Vec<String>,
}

/// Synchronizes a single canister using its configured sync steps
#[allow(clippy::too_many_arguments)]
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
    task: &mut Task,
    pkg_cache: &PackageCache,
) -> Result<Vec<String>, SynchronizeError> {
    let step_count = canister_info.sync.steps.len();
    let mut stderr_lines = Vec::new();

    for (i, step) in canister_info.sync.steps.iter().enumerate() {
        // Indicate to user the current step being executed
        let current_step = i + 1;
        task.begin_step(format!("\nSyncing: {step} {current_step} of {step_count}"));

        // Execute step
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
                Some(task.output()),
                pkg_cache,
            )
            .await;

        task.end_step();

        stderr_lines.extend(sync_result?);
    }

    Ok(stderr_lines)
}

/// Orchestrates syncing multiple canisters, reporting each one's steps.
///
/// `all_step_output` replays every step of a failed sync rather than just the one
/// that failed, which is what `--debug` asks for.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn sync_many(
    syncer: Arc<dyn Synchronize>,
    agent: Agent,
    canisters: Vec<(Principal, PathBuf, Canister)>,
    environment: String,
    network: String,
    canister_ids: BTreeMap<String, Principal>,
    proxy: Option<Principal>,
    reporter: &Reporter,
    all_step_output: bool,
    pkg_cache: &PackageCache,
) -> Result<(), SyncOperationError> {
    let mut futs = FuturesOrdered::new();

    for (cid, canister_path, canister_info) in canisters {
        // Started up front so the tasks appear in the order the canisters were given,
        // regardless of the order the futures below are first polled in.
        let mut task = reporter.task(
            TaskKind::Steps {
                output_label: OUTPUT_LABEL.to_owned(),
            },
            canister_info.name.as_str(),
        );

        let fut = {
            let agent = agent.clone();
            let syncer = syncer.clone();
            let environment = environment.clone();
            let network = network.clone();
            let canister_ids = canister_ids.clone();

            async move {
                // Define the sync logic
                let sync_result = sync_canister(
                    &syncer,
                    &agent,
                    canister_path,
                    cid,
                    &canister_info,
                    &environment,
                    &network,
                    &canister_ids,
                    proxy,
                    &mut task,
                    pkg_cache,
                )
                .await;

                // Read the steps back before the task is consumed, and only when there
                // is a failure to explain.
                let step_output = sync_result.as_ref().err().map(|_| {
                    replay(
                        &canister_info.name,
                        OUTPUT_LABEL,
                        &task.recorded_steps(),
                        all_step_output,
                    )
                });

                let result = task
                    .run(
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
                    step_output: step_output.unwrap_or_default(),
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
            for line in &failure.step_output {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::test_support::{bare_canister, recording_reporter, unreachable_agent};
    use async_trait::async_trait;
    use icp::canister::sync::{Syncer, script::ScriptRunner};
    use icp::manifest::{SyncStep, script};
    use icp_events::{Event, Outcome, TaskId};

    fn pkg_cache() -> (camino_tempfile::Utf8TempDir, PackageCache) {
        let dir = camino_tempfile::Utf8TempDir::new().expect("temp dir");
        let cache = PackageCache::new(dir.path().to_owned()).expect("package cache");
        (dir, cache)
    }

    /// A [`ScriptRunner`] that reports lines of its own instead of running anything,
    /// so the operation can be driven without spawning a shell.
    struct ScriptedRunner {
        lines: Vec<String>,
        fail: bool,
    }

    #[async_trait]
    impl ScriptRunner for ScriptedRunner {
        async fn run_script(
            &self,
            _invocation: icp::canister::sync::script::ScriptInvocation,
            stdio: Option<icp_events::OutputWriter>,
        ) -> Result<Vec<String>, icp::canister::sync::script::ScriptRunError> {
            if let Some(out) = &stdio {
                for line in &self.lines {
                    out.line(line.clone());
                }
            }

            if self.fail {
                return Err(icp::canister::sync::script::ScriptRunError {
                    source: "boom".into(),
                });
            }

            Ok(vec!["retained stderr".to_owned()])
        }
    }

    fn canister_with_a_script_step(name: &str) -> Canister {
        let mut canister = bare_canister(name);
        canister.sync.steps = vec![SyncStep::Script(script::Adapter {
            command: script::CommandField::Command("./deploy.sh".to_owned()),
        })];
        canister
    }

    async fn sync_one(
        runner: ScriptedRunner,
        name: &str,
    ) -> (Vec<Event>, Result<(), SyncOperationError>) {
        let (reporter, sink) = recording_reporter();
        let (_dir, cache) = pkg_cache();
        let cid = Principal::from_slice(&[7; 4]);

        let result = sync_many(
            Arc::new(Syncer::new(Arc::new(runner))),
            unreachable_agent(),
            vec![(cid, "/work".into(), canister_with_a_script_step(name))],
            "local".to_owned(),
            "local".to_owned(),
            BTreeMap::new(),
            None,
            &reporter,
            false,
            &cache,
        )
        .await;

        (sink.events(), result)
    }

    /// The whole shape of a sync, reported: the task, its step, the output the step
    /// produced, and how it ended.
    #[tokio::test]
    async fn a_sync_reports_its_steps_and_their_output() {
        let (events, result) = sync_one(
            ScriptedRunner {
                lines: vec!["uploading assets".to_owned()],
                fail: false,
            },
            "frontend",
        )
        .await;
        result.expect("sync should succeed");

        assert_eq!(
            events.first().unwrap(),
            &Event::TaskStarted {
                id: TaskId(0),
                kind: TaskKind::Steps {
                    output_label: "Sync".to_owned()
                },
                label: Some("frontend".to_owned()),
            }
        );
        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::StepStarted { id, index: 0, title }
                    if *id == TaskId(0) && title.contains("Syncing:") && title.ends_with("1 of 1")
            )),
            "{events:?}"
        );
        assert!(events.contains(&Event::StepOutput {
            id: TaskId(0),
            line: "uploading assets".to_owned(),
        }));
        assert_eq!(
            events.last().unwrap(),
            &Event::TaskFinished {
                id: TaskId(0),
                outcome: Outcome::Success,
                message: Some(format!(
                    "Synced successfully: {}",
                    Principal::from_slice(&[7; 4])
                )),
            }
        );
    }

    #[tokio::test]
    async fn a_failed_sync_reports_the_failure_and_keeps_its_output() {
        let (events, result) = sync_one(
            ScriptedRunner {
                lines: vec!["about to fall over".to_owned()],
                fail: true,
            },
            "frontend",
        )
        .await;
        let err = result.expect_err("sync should fail");
        assert!(err.to_string().contains("frontend"));

        assert!(events.contains(&Event::StepOutput {
            id: TaskId(0),
            line: "about to fall over".to_owned(),
        }));
        let Event::TaskFinished {
            outcome, message, ..
        } = events.last().unwrap().clone()
        else {
            panic!("a sync always finishes its task: {events:?}");
        };
        assert_eq!(outcome, Outcome::Failure);
        assert!(
            message
                .as_deref()
                .is_some_and(|m| m.starts_with("Failed to sync canister:")),
            "{message:?}"
        );
    }

    /// Tasks appear in the order the canisters were given, whichever order their
    /// syncs happen to finish in.
    #[tokio::test]
    async fn tasks_are_started_in_the_order_the_canisters_were_given() {
        use crate::operations::test_support::task_labels;

        let (reporter, sink) = recording_reporter();
        let (_dir, cache) = pkg_cache();
        let cid = Principal::from_slice(&[7; 4]);

        sync_many(
            Arc::new(Syncer::new(Arc::new(ScriptedRunner {
                lines: Vec::new(),
                fail: false,
            }))),
            unreachable_agent(),
            vec![
                (cid, "/work".into(), canister_with_a_script_step("frontend")),
                (cid, "/work".into(), canister_with_a_script_step("backend")),
            ],
            "local".to_owned(),
            "local".to_owned(),
            BTreeMap::new(),
            None,
            &reporter,
            false,
            &cache,
        )
        .await
        .expect("sync should succeed");

        assert_eq!(
            task_labels(&sink.events()),
            vec![Some("frontend".to_owned()), Some("backend".to_owned())]
        );
    }
}
