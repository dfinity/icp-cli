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
    term: &TermWriter,
    debug: bool,
) -> Result<(), anyhow::Error> {
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

            let output = if result.is_err() {
                Some(pb.dump_output())
            } else {
                None
            };

            (result, output)
        };
        futs.push_back(fut);
    }

    // Consume the set of futures and collect results
    let mut failed_outputs = Vec::new();
    while let Some((res, output)) = futs.next().await {
        if let Err(e) = res
            && let Some(output) = output
        {
            failed_outputs.push((e, output));
        }
    }

    if !failed_outputs.is_empty() {
        for (e, output) in failed_outputs {
            for line in output {
                let _ = term.write_line(&line);
            }
            let _ = term.write_line(&format!("Failed to build canister: {e}"));
            let _ = term.write_line("");
        }

        return Err(anyhow::anyhow!("One or more canisters failed to build"));
    }

    Ok(())
}
