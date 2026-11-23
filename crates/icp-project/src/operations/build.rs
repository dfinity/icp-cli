use std::sync::Arc;

use crate::{
    Canister,
    canister::build::{Build, BuildError, Params},
    prelude::*,
};
use futures::{StreamExt, stream::FuturesOrdered};
use icp_events::{StepOutcome, TaskOutcome};

use crate::operations::task::{Reporter, Task, TaskReporter};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum BuildOperationError {
    #[snafu(display("failed to create temporary build directory"))]
    TempDir { source: crate::files::FsError },

    #[snafu(transparent)]
    Build { source: BuildError },

    #[snafu(display("build did not produce a wasm output file"))]
    MissingWasmOutput,

    #[snafu(display("failed to read wasm output file"))]
    ReadWasmOutput { source: crate::files::FsError },

    #[snafu(display("failed to save wasm artifact"))]
    SaveWasmArtifact {
        source: crate::store_artifact::SaveError,
    },
}

#[derive(Debug, Snafu)]
pub enum BuildManyError {
    #[snafu(display("Canister(s) {names:?} failed to build."))]
    BuildMany { names: Vec<String> },

    #[snafu(transparent)]
    SweepArtifacts {
        source: crate::store_artifact::SweepError,
    },
}

pub async fn build(
    canister_path: &Path,
    canister: &Canister,
    environment: &str,
    task: &TaskReporter,
    builder: Arc<dyn Build>,
    artifacts: Arc<dyn crate::store_artifact::Access>,
    files: &dyn crate::files::FileSystem,
) -> Result<(), BuildOperationError> {
    // From `files`, not from this machine: the step writes the module there and
    // the read below comes back through the same seam, so both have to be
    // looking at the same directory.
    let build_dir = files.scratch_dir().await.context(TempDirSnafu)?;
    let wasm_output_path = build_dir.path().join("out.wasm");

    let step_count = canister.build.steps.len();
    for (i, step) in canister.build.steps.iter().enumerate() {
        let reporter = task.step(i + 1, step_count, step.to_string());

        let build_result = builder
            .build(
                step,
                &Params {
                    path: canister_path.to_owned(),
                    output: wasm_output_path.to_owned(),
                    environment: environment.to_owned(),
                },
                &reporter,
            )
            .await;

        reporter.done(match &build_result {
            Ok(()) => StepOutcome::Succeeded,
            Err(_) => StepOutcome::Failed,
        });

        build_result?;
    }

    if !files.exists(&wasm_output_path).await {
        return MissingWasmOutputSnafu.fail();
    }

    let wasm = files
        .read(&wasm_output_path)
        .await
        .context(ReadWasmOutputSnafu)?;

    artifacts
        .save(&canister.name, &wasm)
        .await
        .context(SaveWasmArtifactSnafu)?;

    Ok(())
}

pub async fn build_many(
    canisters: Vec<(PathBuf, Canister)>,
    environment: &str,
    builder: Arc<dyn Build>,
    artifacts: Arc<dyn crate::store_artifact::Access>,
    files: &dyn crate::files::FileSystem,
    reporter: &Reporter,
) -> Result<(), BuildManyError> {
    let mut futs = FuturesOrdered::new();

    for (canister_path, canister) in canisters {
        let task = reporter.task(Task::build(canister.name.clone()));
        let builder = builder.clone();
        let artifacts = artifacts.clone();

        let fut = async move {
            let result = build(
                &canister_path,
                &canister,
                environment,
                &task,
                builder,
                artifacts,
                files,
            )
            .await;

            match &result {
                Ok(()) => task.finish(TaskOutcome::succeeded()),
                Err(error) => task.finish(TaskOutcome::failed(error.to_string())),
            }

            result.map_err(|_| canister.name.clone())
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

    // Once for the whole build rather than after each save, and whether or not
    // every canister built: one that keeps failing does not hold up the rest's
    // cleanup. A failed build is still the error to report.
    let swept = artifacts.sweep().await;

    if !failed.is_empty() {
        return BuildManySnafu { names: failed }.fail();
    }

    Ok(swept?)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;

    use super::{BuildManyError, build_many};
    use crate::{
        canister::build::UnimplementedMockBuilder,
        files::UnimplementedMockFileSystem,
        operations::task::Reporter,
        store_artifact::{Access, LookupArtifactError, SaveError, SweepError},
        store_id::StoreCause,
    };

    /// Counts its sweeps, each of which fails if `fails` is set.
    #[derive(Default)]
    struct Sweeps {
        count: AtomicUsize,
        fails: bool,
    }

    #[async_trait]
    impl Access for Sweeps {
        async fn save(&self, _name: &str, _wasm: &[u8]) -> Result<(), SaveError> {
            unimplemented!()
        }

        async fn lookup(&self, _name: &str) -> Result<Vec<u8>, LookupArtifactError> {
            unimplemented!()
        }

        async fn sweep(&self) -> Result<(), SweepError> {
            self.count.fetch_add(1, Ordering::SeqCst);
            if self.fails {
                return Err(SweepError::SweepStore {
                    source: StoreCause::new(std::io::Error::other("disk gone")),
                });
            }
            Ok(())
        }
    }

    async fn build_nothing(artifacts: Arc<Sweeps>) -> Result<(), BuildManyError> {
        build_many(
            vec![],
            "local",
            Arc::new(UnimplementedMockBuilder),
            artifacts,
            &UnimplementedMockFileSystem,
            &Reporter::null(),
        )
        .await
    }

    #[tokio::test]
    async fn a_build_sweeps_the_artifact_store_once() {
        let artifacts = Arc::new(Sweeps::default());
        build_nothing(artifacts.clone()).await.unwrap();
        assert_eq!(artifacts.count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failed_sweep_fails_the_build() {
        let artifacts = Arc::new(Sweeps {
            fails: true,
            ..Default::default()
        });
        assert!(matches!(
            build_nothing(artifacts).await,
            Err(BuildManyError::SweepArtifacts { .. })
        ));
    }
}
