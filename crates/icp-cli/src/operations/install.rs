use candid::Encode;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, export::Principal};
use ic_management_canister_types::{
    CanisterId, CanisterIdRecord, CanisterInstallMode, CanisterStatusType, ChunkHash,
    ClearChunkStoreArgs, InstallChunkedCodeArgs, InstallCodeArgs, UpgradeFlags, UploadChunkArgs,
    WasmMemoryPersistence,
};
use sha2::{Digest, Sha256};
use snafu::{ResultExt, Snafu};
use std::sync::Arc;
use tracing::{debug, error, warn};

use crate::progress::{ProgressManager, ProgressManagerSettings};

use super::misc::fetch_canister_metadata;
use super::proxy::UpdateOrProxyError;
use super::proxy_management;

#[derive(Debug, Snafu)]
pub enum InstallOperationError {
    #[snafu(display("Could not find build artifact for canister '{canister_name}'"))]
    ArtifactNotFound { canister_name: String },

    #[snafu(display("Failed to stop canister '{canister_name}' before upgrade"))]
    StopCanister {
        canister_name: String,
        source: UpdateOrProxyError,
    },

    #[snafu(display("Failed to start canister '{canister_name}' after upgrade"))]
    StartCanister {
        canister_name: String,
        source: UpdateOrProxyError,
    },

    #[snafu(transparent)]
    UpdateOrProxy { source: UpdateOrProxyError },
}

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to install."))]
pub struct InstallManyError {
    names: Vec<String>,
}

/// Holds error information from a failed canister install operation
struct InstallFailure {
    canister_name: String,
    canister_id: Principal,
    error: InstallOperationError,
}

/// Resolve a mode string ("auto", "install", "reinstall", "upgrade") into
/// a [`CanisterInstallMode`]. For "auto", queries `canister_status` to
/// determine whether the canister already has code installed.
pub(crate) async fn resolve_install_mode_and_status(
    agent: &Agent,
    proxy: Option<Principal>,
    canister_name: &str,
    canister_id: &Principal,
    mode: &str,
) -> Result<(CanisterInstallMode, CanisterStatusType), ResolveInstallModeError> {
    let status = proxy_management::canister_status(
        agent,
        proxy,
        CanisterIdRecord {
            canister_id: CanisterId::from(*canister_id),
        },
    )
    .await
    .context(ResolveInstallModeSnafu { canister_name })?;
    let canister_status = status.status;
    match mode {
        "auto" => Ok(if status.module_hash.is_some() {
            (CanisterInstallMode::Upgrade(None), canister_status)
        } else {
            (CanisterInstallMode::Install, canister_status)
        }),
        "install" => Ok((CanisterInstallMode::Install, canister_status)),
        "reinstall" => Ok((CanisterInstallMode::Reinstall, canister_status)),
        "upgrade" => Ok((CanisterInstallMode::Upgrade(None), canister_status)),
        _ => panic!("invalid install mode: {mode}"),
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("Failed to resolve install mode for canister {canister_name}"))]
pub(crate) struct ResolveInstallModeError {
    canister_name: String,
    source: UpdateOrProxyError,
}

pub(crate) async fn install_canister(
    agent: &Agent,
    proxy: Option<Principal>,
    canister_id: &Principal,
    canister_name: &str,
    wasm: &[u8],
    mode: CanisterInstallMode,
    status: CanisterStatusType,
    init_args: Option<&[u8]>,
) -> Result<(), InstallOperationError> {
    let mode = match mode {
        CanisterInstallMode::Upgrade(_) => {
            // if this is a motoko canister using EOP
            // we need to set additional options
            if fetch_canister_metadata(agent, *canister_id, "enhanced-orthogonal-persistence")
                .await
                .is_some()
            {
                CanisterInstallMode::Upgrade(Some(UpgradeFlags {
                    skip_pre_upgrade: None,
                    wasm_memory_persistence: Some(WasmMemoryPersistence::Keep),
                }))
            } else {
                mode
            }
        }
        _ => mode,
    };

    debug!(
        "Install new canister code for {} with mode `{:?}`",
        canister_name, mode
    );

    do_install_operation(
        agent,
        proxy,
        canister_id,
        canister_name,
        wasm,
        mode,
        status,
        init_args,
    )
    .await
}

async fn do_install_operation(
    agent: &Agent,
    proxy: Option<Principal>,
    canister_id: &Principal,
    canister_name: &str,
    wasm: &[u8],
    mode: CanisterInstallMode,
    status: CanisterStatusType,
    init_args: Option<&[u8]>,
) -> Result<(), InstallOperationError> {
    // Threshold for chunked installation: 2 MB
    // Raw install_code messages are limited to 2 MiB
    const CHUNK_THRESHOLD: usize = 2 * 1024 * 1024;

    // Chunk size: 1 MB (spec limit is 1 MiB per chunk)
    const CHUNK_SIZE: usize = 1024 * 1024;

    // Generous overhead for encoding, target canister ID, install mode, etc.
    const ENCODING_OVERHEAD: usize = 500;

    let cid = CanisterId::from(*canister_id);
    let arg = init_args
        .map(|a| a.to_vec())
        .unwrap_or_else(|| Encode!().unwrap());

    // Calculate total install message size
    let total_install_size = wasm.len() + arg.len() + ENCODING_OVERHEAD;

    if total_install_size <= CHUNK_THRESHOLD {
        // Small wasm: use regular install_code
        debug!("Installing wasm for {canister_name} using install_code");

        let install_args = InstallCodeArgs {
            mode,
            canister_id: cid,
            wasm_module: wasm.to_vec(),
            arg,
            sender_canister_version: None,
        };

        stop_and_start_if_upgrade(
            agent,
            proxy,
            canister_id,
            canister_name,
            mode,
            status,
            async {
                proxy_management::install_code(agent, proxy, install_args).await?;
                Ok(())
            },
        )
        .await?;
    } else {
        // Large wasm: use chunked installation
        debug!("Installing wasm for {canister_name} using chunked installation");

        // Clear any existing chunks to ensure a clean state
        proxy_management::clear_chunk_store(agent, proxy, ClearChunkStoreArgs { canister_id: cid })
            .await?;

        // Split wasm into chunks and upload them
        let chunks: Vec<&[u8]> = wasm.chunks(CHUNK_SIZE).collect();
        let mut chunk_hashes: Vec<ChunkHash> = Vec::new();

        for (i, chunk) in chunks.iter().enumerate() {
            debug!(
                "Uploading chunk {}/{} ({} bytes)",
                i + 1,
                chunks.len(),
                chunk.len()
            );

            let upload_args = UploadChunkArgs {
                canister_id: cid,
                chunk: chunk.to_vec(),
            };

            let chunk_hash = proxy_management::upload_chunk(agent, proxy, upload_args).await?;

            chunk_hashes.push(chunk_hash);
        }

        // Compute SHA-256 hash of the entire wasm module
        let mut hasher = Sha256::new();
        hasher.update(wasm);
        let wasm_module_hash = hasher.finalize().to_vec();

        debug!("Installing chunked code with {} chunks", chunk_hashes.len());

        let chunked_args = InstallChunkedCodeArgs {
            mode,
            target_canister: cid,
            store_canister: None,
            chunk_hashes_list: chunk_hashes,
            wasm_module_hash,
            arg,
            sender_canister_version: None,
        };

        let install_res = stop_and_start_if_upgrade(
            agent,
            proxy,
            canister_id,
            canister_name,
            mode,
            status,
            async {
                proxy_management::install_chunked_code(agent, proxy, chunked_args).await?;
                Ok(())
            },
        )
        .await;

        // Clear chunk store after successful installation to free up storage
        let clear_res = proxy_management::clear_chunk_store(
            agent,
            proxy,
            ClearChunkStoreArgs { canister_id: cid },
        )
        .await
        .map_err(InstallOperationError::from);

        if let Err(clear_error) = clear_res {
            if let Err(install_error) = install_res {
                warn!("Failed to clear chunk store after failed install: {clear_error}");
                return Err(install_error);
            } else {
                return Err(clear_error);
            }
        }
        install_res?;
    }

    Ok(())
}

async fn stop_and_start_if_upgrade(
    agent: &Agent,
    proxy: Option<Principal>,
    canister_id: &Principal,
    canister_name: &str,
    mode: CanisterInstallMode,
    status: CanisterStatusType,
    f: impl Future<Output = Result<(), InstallOperationError>>,
) -> Result<(), InstallOperationError> {
    let should_guard = matches!(
        mode,
        CanisterInstallMode::Upgrade(_) | CanisterInstallMode::Reinstall
    ) && matches!(status, CanisterStatusType::Running);
    let cid_record = CanisterIdRecord {
        canister_id: CanisterId::from(*canister_id),
    };
    // Stop the canister before proceeding
    if should_guard {
        proxy_management::stop_canister(agent, proxy, cid_record.clone())
            .await
            .context(StopCanisterSnafu { canister_name })?;
    }
    // Install the canister
    let install_result = f.await;
    // Restart the canister whether or not the installation succeeded
    if should_guard {
        let start_result = proxy_management::start_canister(agent, proxy, cid_record).await;
        if let Err(start_error) = start_result {
            // If both install and start failed, report the install error since it's more likely to be the root cause
            if let Err(install_error) = install_result {
                warn!("Failed to start canister after failed upgrade: {start_error}");
                return Err(install_error);
            } else {
                return Err(start_error).context(StartCanisterSnafu { canister_name });
            }
        }
    }

    install_result
}

/// Installs code to multiple canisters and displays progress bars.
pub(crate) async fn install_many(
    agent: Agent,
    proxy: Option<Principal>,
    canisters: impl IntoIterator<
        Item = (
            String,
            Principal,
            CanisterInstallMode,
            CanisterStatusType,
            Option<Vec<u8>>,
        ),
    >,
    artifacts: Arc<dyn icp::store_artifact::Access>,
    debug: bool,
) -> Result<(), InstallManyError> {
    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for (name, cid, mode, status, init_args) in canisters {
        let pb = progress_manager.create_progress_bar(&name);
        let agent = agent.clone();
        let install_fn = {
            let pb = pb.clone();
            let artifacts = artifacts.clone();
            let name = name.clone();

            async move {
                pb.set_message("Installing...");

                let wasm = artifacts.lookup(&name).await.map_err(|_| {
                    InstallOperationError::ArtifactNotFound {
                        canister_name: name.clone(),
                    }
                })?;

                install_canister(
                    &agent,
                    proxy,
                    &cid,
                    &name,
                    &wasm,
                    mode,
                    status,
                    init_args.as_deref(),
                )
                .await
            }
        };

        futs.push_back(async move {
            let result = ProgressManager::execute_with_progress(
                &pb,
                install_fn,
                || "Installed successfully".to_string(),
                |err| format!("Failed to install canister: {err}"),
            )
            .await;

            result.map_err(|error| InstallFailure {
                canister_name: name.clone(),
                canister_id: cid,
                error,
            })
        });
    }

    let mut errors: Vec<InstallFailure> = Vec::new();
    while let Some(res) = futs.next().await {
        if let Err(failure) = res {
            errors.push(failure);
        }
    }

    if !errors.is_empty() {
        for failure in &errors {
            error!(
                "----- Failed to install canister '{}': {} -----",
                failure.canister_name, failure.canister_id,
            );
            error!("'{}'", failure.error);
        }

        return InstallManySnafu {
            names: errors
                .iter()
                .map(|e| e.canister_name.clone())
                .collect::<Vec<String>>(),
        }
        .fail();
    }

    Ok(())
}
