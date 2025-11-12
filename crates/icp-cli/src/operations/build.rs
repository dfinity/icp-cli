use std::sync::Arc;

use camino_tempfile::tempdir;
use icp::{
    Canister,
    canister::build::{Build, BuildError, Params},
    prelude::*,
};
use snafu::{ResultExt, Snafu};

use crate::progress::MultiStepProgressBar;

#[derive(Debug, Snafu)]
pub enum BuildOperationError {
    #[snafu(display("failed to create temporary build directory"))]
    TempDir { source: std::io::Error },

    #[snafu(transparent)]
    Build { source: BuildError },

    #[snafu(display("build did not produce a wasm output file"))]
    MissingWasmOutput,

    #[snafu(display("failed to read wasm output file"))]
    ReadWasmOutput { source: icp::fs::Error },

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
        return Err(BuildOperationError::MissingWasmOutput);
    }

    let wasm = icp::fs::read(&wasm_output_path).context(ReadWasmOutputSnafu)?;

    artifacts
        .save(&canister.name, &wasm)
        .await
        .context(SaveWasmArtifactSnafu)?;

    Ok(())
}

// pub(crate) async fn build_many_with_progress_bar(
//     canisters: Vec<(PathBuf, Canister)>,
//     builder: Arc<dyn Build>,
//     artifacts: Arc<dyn icp::store_artifact::Access>,
//     debug: bool,
// ) -> Result<(), anyhow::Error> {
//     let futs = FuturesOrdered::new();
//     let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

//     for (canister_path, canister) in canisters {
//         let mut pb = progress_manager.create_multi_step_progress_bar(&canister.name, "Build");
//         let fut =  async move {
//             let build_result = build(&canister_path, &canister, &mut pb, builder, artifacts).await;
//             futs.push_back(fut);
//         };
//     }

//     todo!()
// }
