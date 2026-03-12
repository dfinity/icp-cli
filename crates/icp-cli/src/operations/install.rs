use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, AgentError, export::Principal};
use ic_management_canister_types::{
    CanisterId, ChunkHash, UpgradeFlags, UploadChunkArgs, WasmMemoryPersistence,
};
use ic_utils::interfaces::{
    ManagementCanister, management_canister::builders::CanisterInstallMode,
};
use sha2::{Digest, Sha256};
use snafu::{ResultExt, Snafu};
use std::sync::Arc;
use tracing::{debug, error};

use crate::progress::{ProgressManager, ProgressManagerSettings};

use super::misc::fetch_canister_metadata;

#[derive(Debug, Snafu)]
pub enum InstallOperationError {
    #[snafu(display("Could not find build artifact for canister '{canister_name}'"))]
    ArtifactNotFound { canister_name: String },

    #[snafu(display("agent error: {source}"))]
    Agent { source: AgentError },
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
pub(crate) async fn resolve_install_mode(
    agent: &Agent,
    canister_name: &str,
    canister_id: &Principal,
    mode: &str,
) -> Result<CanisterInstallMode, ResolveInstallModeError> {
    match mode {
        "auto" => {
            let mgmt = ManagementCanister::create(agent);
            let (status,) = mgmt
                .canister_status(canister_id)
                .await
                .context(ResolveInstallModeSnafu { canister_name })?;
            Ok(if status.module_hash.is_some() {
                CanisterInstallMode::Upgrade(None)
            } else {
                CanisterInstallMode::Install
            })
        }
        "install" => Ok(CanisterInstallMode::Install),
        "reinstall" => Ok(CanisterInstallMode::Reinstall),
        "upgrade" => Ok(CanisterInstallMode::Upgrade(None)),
        _ => panic!("invalid install mode: {mode}"),
    }
}

#[derive(Debug, Snafu)]
#[snafu(display("Failed to resolve install mode for canister {canister_name}"))]
pub(crate) struct ResolveInstallModeError {
    canister_name: String,
    source: AgentError,
}

pub(crate) async fn install_canister(
    agent: &Agent,
    canister_id: &Principal,
    canister_name: &str,
    wasm: &[u8],
    mode: CanisterInstallMode,
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

    do_install_operation(agent, canister_id, canister_name, wasm, mode, init_args).await
}

async fn do_install_operation(
    agent: &Agent,
    canister_id: &Principal,
    canister_name: &str,
    wasm: &[u8],
    mode: CanisterInstallMode,
    init_args: Option<&[u8]>,
) -> Result<(), InstallOperationError> {
    let mgmt = ManagementCanister::create(agent);

    // Threshold for chunked installation: 2 MB
    // Raw install_code messages are limited to 2 MiB
    const CHUNK_THRESHOLD: usize = 2 * 1024 * 1024;

    // Chunk size: 1 MB (spec limit is 1 MiB per chunk)
    const CHUNK_SIZE: usize = 1024 * 1024;

    // Generous overhead for encoding, target canister ID, install mode, etc.
    const ENCODING_OVERHEAD: usize = 500;

    // Calculate total install message size
    let init_args_len = init_args.map_or(0, |args| args.len());
    let total_install_size = wasm.len() + init_args_len + ENCODING_OVERHEAD;

    if total_install_size <= CHUNK_THRESHOLD {
        // Small wasm: use regular install_code
        debug!("Installing wasm for {canister_name} using install_code");

        let mut builder = mgmt.install_code(canister_id, wasm).with_mode(mode);

        if let Some(args) = init_args {
            builder = builder.with_raw_arg(args.into());
        }

        builder
            .await
            .map_err(|source| InstallOperationError::Agent { source })?;
    } else {
        // Large wasm: use chunked installation
        debug!("Installing wasm for {canister_name} using chunked installation");

        // Clear any existing chunks to ensure a clean state
        mgmt.clear_chunk_store(canister_id)
            .await
            .map_err(|source| InstallOperationError::Agent { source })?;

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
                canister_id: CanisterId::from(*canister_id),
                chunk: chunk.to_vec(),
            };

            let (chunk_hash,) = mgmt
                .upload_chunk(canister_id, &upload_args)
                .await
                .map_err(|source| InstallOperationError::Agent { source })?;

            chunk_hashes.push(chunk_hash);
        }

        // Compute SHA-256 hash of the entire wasm module
        let mut hasher = Sha256::new();
        hasher.update(wasm);
        let wasm_module_hash = hasher.finalize().to_vec();

        debug!("Installing chunked code with {} chunks", chunk_hashes.len());

        // Build and execute install_chunked_code
        let mut builder = mgmt
            .install_chunked_code(canister_id, &wasm_module_hash)
            .with_chunk_hashes(chunk_hashes)
            .with_install_mode(mode);

        if let Some(args) = init_args {
            builder = builder.with_raw_arg(args.to_vec());
        }

        builder
            .await
            .map_err(|source| InstallOperationError::Agent { source })?;

        // Clear chunk store after successful installation to free up storage
        mgmt.clear_chunk_store(canister_id)
            .await
            .map_err(|source| InstallOperationError::Agent { source })?;
    }

    Ok(())
}

/// Installs code to multiple canisters and displays progress bars.
pub(crate) async fn install_many(
    agent: Agent,
    canisters: impl IntoIterator<Item = (String, Principal, CanisterInstallMode, Option<Vec<u8>>)>,
    artifacts: Arc<dyn icp::store_artifact::Access>,
    debug: bool,
) -> Result<(), InstallManyError> {
    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for (name, cid, mode, init_args) in canisters {
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

                install_canister(&agent, &cid, &name, &wasm, mode, init_args.as_deref()).await
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
                " ----- Failed to install canister '{}': {} -----",
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
