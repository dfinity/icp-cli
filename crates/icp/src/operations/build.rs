use std::sync::Arc;

use crate::{
    Canister,
    canister::build::{Build, BuildError, Params},
    package::PackageCache,
    prelude::*,
};
use camino_tempfile::tempdir;
use futures::{StreamExt, stream::FuturesOrdered};
use icp_events::{Reporter, StepOutcome, TaskKind, TaskOutcome, TaskReporter};
use snafu::{ResultExt, Snafu};

#[derive(Debug, Snafu)]
pub enum BuildOperationError {
    #[snafu(display("failed to create temporary build directory"))]
    TempDir { source: std::io::Error },

    #[snafu(transparent)]
    Build { source: BuildError },

    #[snafu(display("build did not produce a wasm output file"))]
    MissingWasmOutput,

    #[snafu(display("failed to read wasm output file"))]
    ReadWasmOutput { source: crate::fs::IoError },

    #[snafu(display("failed to save wasm artifact"))]
    SaveWasmArtifact {
        source: crate::store_artifact::SaveError,
    },
}

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to build."))]
pub struct BuildManyError {
    names: Vec<String>,
}

pub async fn build(
    canister_path: &Path,
    canister: &Canister,
    environment: &str,
    task: &TaskReporter,
    builder: Arc<dyn Build>,
    artifacts: Arc<dyn crate::store_artifact::Access>,
    pkg_cache: &PackageCache,
) -> Result<(), BuildOperationError> {
    let build_dir = tempdir().context(TempDirSnafu)?;
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
                pkg_cache,
            )
            .await;

        reporter.done(match &build_result {
            Ok(()) => StepOutcome::Succeeded,
            Err(_) => StepOutcome::Failed,
        });

        build_result?;
    }

    if !wasm_output_path.exists() {
        return MissingWasmOutputSnafu.fail();
    }

    let wasm = crate::fs::read(&wasm_output_path).context(ReadWasmOutputSnafu)?;

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
    pkg_cache: &PackageCache,
    reporter: &Reporter,
) -> Result<(), BuildManyError> {
    let mut futs = FuturesOrdered::new();

    for (canister_path, canister) in canisters {
        let task = reporter.task(TaskKind::Build {
            canister: canister.name.clone(),
        });
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
                pkg_cache,
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

    if !failed.is_empty() {
        return BuildManySnafu { names: failed }.fail();
    }

    Ok(())
}
