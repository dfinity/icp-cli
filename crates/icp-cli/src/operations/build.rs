use std::sync::Arc;

use camino_tempfile::tempdir;
use futures::{StreamExt, stream::FuturesOrdered};
use icp::{
    Canister,
    canister::build::{Build, BuildError, Params},
    package::PackageCache,
    prelude::*,
};
use icp_events::{Reporter, Task, TaskKind};
use snafu::{ResultExt, Snafu};
use tracing::error;

use crate::operations::step_replay::replay;

/// What a build task's output is called when it is replayed after a failure.
const OUTPUT_LABEL: &str = "Build";

#[derive(Debug, Snafu)]
pub enum BuildOperationError {
    #[snafu(display("failed to create temporary build directory"))]
    TempDir { source: std::io::Error },

    #[snafu(transparent)]
    Build { source: BuildError },

    #[snafu(display("build did not produce a wasm output file"))]
    MissingWasmOutput,

    #[snafu(display("failed to read wasm output file"))]
    ReadWasmOutput { source: icp::fs::IoError },

    #[snafu(display("failed to save wasm artifact"))]
    SaveWasmArtifact {
        source: icp::store_artifact::SaveError,
    },
}

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to build."))]
pub struct BuildManyError {
    names: Vec<String>,
}

/// Holds error information from a failed canister build operation
struct BuildFailure {
    canister_name: String,
    error: BuildOperationError,
    step_output: Vec<String>,
}

pub(crate) async fn build(
    canister_path: &Path,
    canister: &Canister,
    environment: &str,
    task: &mut Task,
    builder: Arc<dyn Build>,
    artifacts: Arc<dyn icp::store_artifact::Access>,
    pkg_cache: &PackageCache,
) -> Result<(), BuildOperationError> {
    let build_dir = tempdir().context(TempDirSnafu)?;
    let wasm_output_path = build_dir.path().join("out.wasm");

    let step_count = canister.build.steps.len();
    for (i, step) in canister.build.steps.iter().enumerate() {
        let current_step = i + 1;
        task.begin_step(format!(
            "Building: step {current_step} of {step_count} {step}"
        ));

        let build_result = builder
            .build(
                step,
                &Params {
                    path: canister_path.to_owned(),
                    output: wasm_output_path.to_owned(),
                    environment: environment.to_owned(),
                },
                Some(task.output()),
                pkg_cache,
            )
            .await;

        task.end_step();

        build_result?;
    }

    if !wasm_output_path.exists() {
        return MissingWasmOutputSnafu.fail();
    }

    let wasm = icp::fs::read(&wasm_output_path).context(ReadWasmOutputSnafu)?;

    artifacts
        .save(&canister.name, &wasm)
        .await
        .context(SaveWasmArtifactSnafu)?;

    Ok(())
}

/// Builds several canisters, reporting each one's steps and their output.
///
/// `all_step_output` replays every step of a failed build rather than just the one
/// that failed, which is what `--debug` asks for.
pub(crate) async fn build_many(
    canisters: Vec<(PathBuf, Canister)>,
    environment: &str,
    builder: Arc<dyn Build>,
    artifacts: Arc<dyn icp::store_artifact::Access>,
    pkg_cache: &PackageCache,
    reporter: &Reporter,
    all_step_output: bool,
) -> Result<(), BuildManyError> {
    let mut futs = FuturesOrdered::new();

    for (canister_path, canister) in canisters {
        // Started up front so the tasks appear in the order the canisters were given,
        // regardless of the order the futures below are first polled in.
        let mut task = reporter.task(
            TaskKind::Steps {
                output_label: OUTPUT_LABEL.to_owned(),
            },
            canister.name.as_str(),
        );
        let builder = builder.clone();
        let artifacts = artifacts.clone();
        let fut = async move {
            let build_result = build(
                &canister_path,
                &canister,
                environment,
                &mut task,
                builder,
                artifacts,
                pkg_cache,
            )
            .await;

            // Read the steps back before the task is consumed, and only when there is
            // a failure to explain.
            let step_output = build_result.as_ref().err().map(|_| {
                replay(
                    &canister.name,
                    OUTPUT_LABEL,
                    &task.recorded_steps(),
                    all_step_output,
                )
            });

            let result = task
                .run(
                    async { build_result },
                    || "Built successfully".to_string(),
                    |err| format!("Failed to build canister: {err}"),
                )
                .await;

            // Map error to include canister context for deferred printing
            result.map_err(|error| BuildFailure {
                canister_name: canister.name.clone(),
                error,
                step_output: step_output.unwrap_or_default(),
            })
        };
        futs.push_back(fut);
    }

    // Consume the set of futures and collect errors
    let mut errors: Vec<BuildFailure> = Vec::new();
    while let Some(res) = futs.next().await {
        if let Err(failure) = res {
            errors.push(failure);
        }
    }

    if !errors.is_empty() {
        // Print all errors in batch
        for failure in &errors {
            error!(
                "----- Failed to build canister '{}' -----",
                failure.canister_name,
            );
            error!("'{}'", failure.error);
            for line in &failure.step_output {
                error!("{line}");
            }
        }

        return BuildManySnafu {
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
    use crate::operations::test_support::{
        EmptyArtifacts, bare_canister, recording_reporter, task_labels,
    };
    use icp::manifest::{BuildStep, script};
    use icp_events::{Event, Outcome, TaskId};

    fn pkg_cache() -> (camino_tempfile::Utf8TempDir, PackageCache) {
        let dir = camino_tempfile::Utf8TempDir::new().expect("temp dir");
        let cache = PackageCache::new(dir.path().to_owned()).expect("package cache");
        (dir, cache)
    }

    /// A canister with one script step that prints a line and writes the wasm the
    /// build then looks for.
    fn canister_printing(name: &str, line: &str) -> Canister {
        let mut canister = bare_canister(name);
        canister.build.steps = vec![BuildStep::Script(script::Adapter {
            command: script::CommandField::Command(format!(
                r#"echo {line} && echo wasm > "$ICP_WASM_OUTPUT_PATH""#
            )),
        })];
        canister
    }

    /// A canister whose one script step fails.
    fn canister_failing(name: &str) -> Canister {
        let mut canister = bare_canister(name);
        canister.build.steps = vec![BuildStep::Script(script::Adapter {
            command: script::CommandField::Command("echo doomed && exit 3".to_owned()),
        })];
        canister
    }

    /// The whole shape of a build, reported: the task, its step, the output the step
    /// produced, and how it ended.
    #[tokio::test]
    async fn a_build_reports_its_steps_and_their_output() {
        let (reporter, sink) = recording_reporter();
        let (_dir, cache) = pkg_cache();
        let out = camino_tempfile::Utf8TempDir::new().expect("temp dir");

        build_many(
            vec![(out.path().to_owned(), canister_printing("backend", "hello"))],
            "local",
            Arc::new(icp::canister::build::Builder),
            Arc::new(EmptyArtifacts),
            &cache,
            &reporter,
            false,
        )
        .await
        .expect("build should succeed");

        let events = sink.events();
        assert_eq!(
            events.first().unwrap(),
            &Event::TaskStarted {
                id: TaskId(0),
                kind: TaskKind::Steps {
                    output_label: "Build".to_owned()
                },
                label: Some("backend".to_owned()),
            }
        );
        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::StepStarted { id, index: 0, title }
                    if *id == TaskId(0) && title.starts_with("Building: step 1 of 1")
            )),
            "{events:?}"
        );
        assert!(events.contains(&Event::StepOutput {
            id: TaskId(0),
            line: "hello".to_owned(),
        }));
        assert_eq!(
            events.last().unwrap(),
            &Event::TaskFinished {
                id: TaskId(0),
                outcome: Outcome::Success,
                message: Some("Built successfully".to_owned()),
            }
        );
    }

    /// A failing step is reported as a failure, and its output is available for the
    /// replay that follows.
    #[tokio::test]
    async fn a_failed_build_reports_the_failure_and_keeps_its_output() {
        let (reporter, sink) = recording_reporter();
        let (_dir, cache) = pkg_cache();
        let out = camino_tempfile::Utf8TempDir::new().expect("temp dir");

        let err = build_many(
            vec![(out.path().to_owned(), canister_failing("backend"))],
            "local",
            Arc::new(icp::canister::build::Builder),
            Arc::new(EmptyArtifacts),
            &cache,
            &reporter,
            false,
        )
        .await
        .expect_err("build should fail");
        assert!(err.to_string().contains("backend"));

        let events = sink.events();
        assert!(events.contains(&Event::StepOutput {
            id: TaskId(0),
            line: "doomed".to_owned(),
        }));
        let Event::TaskFinished {
            outcome, message, ..
        } = events.last().unwrap().clone()
        else {
            panic!("a build always finishes its task: {events:?}");
        };
        assert_eq!(outcome, Outcome::Failure);
        assert!(
            message
                .as_deref()
                .is_some_and(|m| m.starts_with("Failed to build canister:")),
            "{message:?}"
        );
    }

    /// Tasks appear in the order the canisters were given, whichever order their
    /// builds happen to finish in.
    #[tokio::test]
    async fn tasks_are_started_in_the_order_the_canisters_were_given() {
        let (reporter, sink) = recording_reporter();
        let (_dir, cache) = pkg_cache();
        let out = camino_tempfile::Utf8TempDir::new().expect("temp dir");

        let canisters = vec![
            (
                out.path().to_owned(),
                canister_printing("frontend", "first"),
            ),
            (
                out.path().to_owned(),
                canister_printing("backend", "second"),
            ),
        ];
        build_many(
            canisters,
            "local",
            Arc::new(icp::canister::build::Builder),
            Arc::new(EmptyArtifacts),
            &cache,
            &reporter,
            false,
        )
        .await
        .expect("build should succeed");

        assert_eq!(
            task_labels(&sink.events()),
            vec![Some("frontend".to_owned()), Some("backend".to_owned())]
        );
    }
}
