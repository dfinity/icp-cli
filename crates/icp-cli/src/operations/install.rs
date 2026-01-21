use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, AgentError, export::Principal};
use ic_management_canister_types::{
    CanisterId, ChunkHash, UpgradeFlags, UploadChunkArgs, WasmMemoryPersistence,
};
use ic_utils::interfaces::{
    ManagementCanister, management_canister::builders::CanisterInstallMode,
};
use sha2::{Digest, Sha256};
use snafu::Snafu;
use std::sync::Arc;
use tracing::debug;

use crate::progress::{ProgressManager, ProgressManagerSettings};

use super::misc::fetch_canister_metadata;

#[derive(Debug, Snafu)]
pub enum InstallOperationError {
    #[snafu(display("Could not find build artifact for canister '{canister_name}'"))]
    ArtifactNotFound { canister_name: String },

    #[snafu(display("agent error: {source}"))]
    Agent { source: AgentError },
}

pub(crate) async fn install_canister(
    agent: &Agent,
    canister_id: &Principal,
    canister_name: &str,
    wasm: &[u8],
    mode: &str,
    init_args: Option<&[u8]>,
) -> Result<(), InstallOperationError> {
    let mgmt = ManagementCanister::create(agent);
    let install_mode = match mode {
        "auto" => {
            let (status,) = mgmt
                .canister_status(canister_id)
                .await
                .map_err(|source| InstallOperationError::Agent { source })?;

            match status.module_hash {
                // Canister has had code installed to it.
                Some(_) => CanisterInstallMode::Upgrade(None),

                // Canister has not had code installed to it.
                None => CanisterInstallMode::Install,
            }
        }
        "install" => CanisterInstallMode::Install,
        "reinstall" => CanisterInstallMode::Reinstall,
        "upgrade" => CanisterInstallMode::Upgrade(None),
        _ => panic!("invalid install mode"),
    };

    let install_mode = match install_mode {
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
                install_mode
            }
        }
        _ => install_mode,
    };

    // Install code to canister
    debug!(
        "Install new canister code for {} with mode `{:?}`",
        canister_name, install_mode
    );

    do_install_operation(
        agent,
        canister_id,
        canister_name,
        wasm,
        install_mode,
        init_args,
    )
    .await
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

/// Installs code to multiple canisters and displays progress bars
pub(crate) async fn install_many(
    agent: Agent,
    canisters: Vec<(String, Principal, Option<Vec<u8>>)>,
    mode: &str,
    artifacts: Arc<dyn icp::store_artifact::Access>,
    debug: bool,
) -> Result<(), InstallOperationError> {
    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for (name, cid, init_args) in canisters {
        let pb = progress_manager.create_progress_bar(&name);
        let agent = agent.clone();
        let install_fn = {
            let pb = pb.clone();
            let mode = mode.to_string();
            let artifacts = artifacts.clone();
            let name = name.clone();

            async move {
                pb.set_message("Installing...");

                // Lookup the canister build artifact
                let wasm = artifacts.lookup(&name).await.map_err(|_| {
                    InstallOperationError::ArtifactNotFound {
                        canister_name: name.clone(),
                    }
                })?;

                install_canister(&agent, &cid, &name, &wasm, &mode, init_args.as_deref()).await
            }
        };

        futs.push_back(async move {
            ProgressManager::execute_with_progress(
                &pb,
                install_fn,
                || "Installed successfully".to_string(),
                |err| format!("Failed to install canister: {err}"),
            )
            .await
        });
    }

    while let Some(res) = futs.next().await {
        res?;
    }

    Ok(())
}
