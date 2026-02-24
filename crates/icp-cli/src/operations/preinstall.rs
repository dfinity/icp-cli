use std::sync::Arc;

use camino_tempfile::tempdir;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::export::Principal;
use icp::{
    Canister,
    canister::preinstall::{Params, Preinstall, PreinstallError},
    context::TermWriter,
    prelude::*,
};
use snafu::{IntoError, ResultExt, Snafu};

use crate::progress::{MultiStepProgressBar, ProgressManager, ProgressManagerSettings};

#[derive(Debug, Snafu)]
pub enum PreinstallOperationError {
    #[snafu(display("failed to create temporary directory"))]
    TempDir { source: std::io::Error },

    #[snafu(display("failed to write WASM to temporary file"))]
    WriteWasm { source: icp::fs::IoError },

    #[snafu(display("failed to lookup WASM artifact for canister"))]
    LookupArtifact {
        source: icp::store_artifact::LookupArtifactError,
    },

    #[snafu(transparent)]
    Preinstall { source: PreinstallError },
}

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed preinstall checks."))]
pub struct PreinstallManyError {
    names: Vec<String>,
}

struct PreinstallFailure {
    canister_name: String,
    canister_id: Principal,
    error: PreinstallOperationError,
    progress_output: Vec<String>,
}

async fn preinstall_canister(
    preinstaller: &Arc<dyn Preinstall>,
    _canister_name: &str,
    canister_id: Principal,
    canister_path: PathBuf,
    canister_info: &Canister,
    wasm: &[u8],
    environment: &str,
    pb: &mut MultiStepProgressBar,
) -> Result<(), PreinstallOperationError> {
    // Write WASM to temp file for preinstall script
    let temp_dir = tempdir().context(TempDirSnafu)?;
    let wasm_path = temp_dir.path().join("canister.wasm");
    icp::fs::write(&wasm_path, wasm).context(WriteWasmSnafu)?;

    let step_count = canister_info.preinstall.steps.len();
    for (i, step) in canister_info.preinstall.steps.iter().enumerate() {
        let current_step = i + 1;
        let pb_hdr = format!("Preinstall: step {current_step} of {step_count} {step}");
        let tx = pb.begin_step(pb_hdr);

        let preinstall_result = preinstaller
            .preinstall(
                step,
                &Params {
                    path: canister_path.clone(),
                    wasm_path: wasm_path.clone(),
                    cid: canister_id,
                    environment: environment.to_string(),
                },
                Some(tx),
            )
            .await;

        pb.end_step().await;

        preinstall_result?;
    }

    Ok(())
}

pub(crate) async fn preinstall_many(
    preinstaller: Arc<dyn Preinstall>,
    artifacts: Arc<dyn icp::store_artifact::Access>,
    term: Arc<TermWriter>,
    canisters: Vec<(String, Principal, PathBuf, Canister, String)>,
    debug: bool,
) -> Result<(), PreinstallManyError> {
    // Filter canisters with preinstall steps
    let preinstall_canisters: Vec<_> = canisters
        .into_iter()
        .filter(|(_, _, _, info, _)| !info.preinstall.steps.is_empty())
        .collect();

    if preinstall_canisters.is_empty() {
        return Ok(());
    }

    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for (name, cid, canister_path, canister_info, environment) in preinstall_canisters {
        let mut pb = progress_manager.create_multi_step_progress_bar(&name, "Preinstall");
        let preinstaller = preinstaller.clone();
        let artifacts = artifacts.clone();

        let fut = async move {
            // Lookup WASM artifact
            let wasm = match artifacts.lookup(&name).await {
                Ok(w) => w,
                Err(e) => {
                    let error = LookupArtifactSnafu.into_error(e);
                    return Err(PreinstallFailure {
                        canister_name: name.clone(),
                        canister_id: cid,
                        error,
                        progress_output: pb.dump_output(debug),
                    });
                }
            };

            let preinstall_result = preinstall_canister(
                &preinstaller,
                &name,
                cid,
                canister_path,
                &canister_info,
                &wasm,
                &environment,
                &mut pb,
            )
            .await;

            let result = ProgressManager::execute_with_progress(
                &pb,
                async { preinstall_result },
                || "Preinstall checks passed".to_string(),
                |err| format!("Preinstall check failed: {err}"),
            )
            .await;

            result.map_err(|error| PreinstallFailure {
                canister_name: name.clone(),
                canister_id: cid,
                error,
                progress_output: pb.dump_output(debug),
            })
        };
        futs.push_back(fut);
    }

    // Collect all errors
    let mut errors: Vec<PreinstallFailure> = Vec::new();
    while let Some(res) = futs.next().await {
        if let Err(failure) = res {
            errors.push(failure);
        }
    }

    if !errors.is_empty() {
        // Print all errors in batch
        for failure in &errors {
            let _ = term.write_line("");
            let _ = term.write_line("");
            let _ = term.write_line(&format!(
                " ----- Preinstall failed for canister '{}' ({}) -----",
                failure.canister_name, failure.canister_id,
            ));
            let _ = term.write_line(&format!("Error: '{}'", failure.error));
            for line in &failure.progress_output {
                let _ = term.write_line(line);
            }
            let _ = term.write_line("");
        }

        return PreinstallManySnafu {
            names: errors
                .iter()
                .map(|e| e.canister_name.clone())
                .collect::<Vec<String>>(),
        }
        .fail();
    }

    Ok(())
}
