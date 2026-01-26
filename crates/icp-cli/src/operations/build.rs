use std::sync::Arc;

use camino_tempfile::tempdir;
use futures::{StreamExt, stream::FuturesOrdered};
use icp::{
    Canister,
    canister::build::{Build, BuildError, Params},
    context::TermWriter,
    prelude::*,
};
use snafu::{ResultExt, Snafu};

use crate::progress::{MultiStepProgressBar, ProgressManager, ProgressManagerSettings};

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
    progress_output: Vec<String>,
}

pub(crate) async fn build(
    canister_path: &Path,
    canister: &Canister,
    pb: &mut MultiStepProgressBar,
    builder: Arc<dyn Build>,
    artifacts: Arc<dyn icp::store_artifact::Access>,
) -> Result<(), BuildOperationError> {
    let build_dir = tempdir().context(TempDirSnafu)?;
    let wasm_output_path = build_dir.path().join("out.wasm");

    let step_count = canister.build.steps.len();
    for (i, step) in canister.build.steps.iter().enumerate() {
        let current_step = i + 1;
        let pb_hdr = format!("Building: step {current_step} of {step_count} {step}");
        let tx = pb.begin_step(pb_hdr);

        let build_result = builder
            .build(
                step,
                &Params {
                    path: canister_path.to_owned(),
                    output: wasm_output_path.to_owned(),
                },
                Some(tx),
            )
            .await;

        pb.end_step().await;

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

pub(crate) async fn build_many_with_progress_bar(
    canisters: Vec<(PathBuf, Canister)>,
    builder: Arc<dyn Build>,
    artifacts: Arc<dyn icp::store_artifact::Access>,
    term: Arc<TermWriter>,
    debug: bool,
) -> Result<(), BuildManyError> {
    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for (canister_path, canister) in canisters {
        let mut pb = progress_manager.create_multi_step_progress_bar(&canister.name, "Build");
        let builder = builder.clone();
        let artifacts = artifacts.clone();
        let fut = async move {
            let build_result = build(&canister_path, &canister, &mut pb, builder, artifacts).await;

            // Execute with progress tracking for final state
            let result = ProgressManager::execute_with_progress(
                &pb,
                async { build_result },
                || "Built successfully".to_string(),
                |err| format!("Failed to build canister: {err}"),
            )
            .await;

            // Map error to include canister context for deferred printing
            result.map_err(|error| BuildFailure {
                canister_name: canister.name.clone(),
                error,
                progress_output: pb.dump_output(),
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
            // Print progress output
            let _ = term.write_line("");
            let _ = term.write_line("");
            let _ = term.write_line(&format!(
                " ----- Failed to build canister '{}' -----",
                failure.canister_name,
            ));
            let _ = term.write_line(&format!("Error: '{}'", failure.error));
            for line in &failure.progress_output {
                let _ = term.write_line(line);
            }

            let _ = term.write_line("");
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
