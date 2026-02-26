use candid::types::subtype::{OptReport, subtype_with_config};
use candid_parser::utils::CandidSource;
use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, AgentError, export::Principal};
use ic_management_canister_types::{
    CanisterId, ChunkHash, UpgradeFlags, UploadChunkArgs, WasmMemoryPersistence,
};
use ic_utils::interfaces::{
    ManagementCanister, management_canister::builders::CanisterInstallMode,
};
use icp::context::TermWriter;
use sha2::{Digest, Sha256};
use snafu::Snafu;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::debug;

use crate::progress::{ProgressManager, ProgressManagerSettings};

use super::misc::fetch_canister_metadata;
use super::wasm::extract_candid_service;

#[derive(Debug, Snafu)]
pub enum InstallOperationError {
    #[snafu(display("Could not find build artifact for canister '{canister_name}'"))]
    ArtifactNotFound { canister_name: String },

    #[snafu(display("agent error: {source}"))]
    Agent { source: AgentError },

    #[snafu(display(
        "Candid interface compatibility check failed for canister '{canister_name}'.\n\
         You are making a BREAKING change. Other canisters or frontend clients \
         relying on your canister may stop working.\n\n\
         {details}\n\n\
         Use --yes to bypass this check."
    ))]
    CandidIncompatible {
        canister_name: String,
        details: String,
    },
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

/// Holds error information from a failed candid compatibility check
struct CandidCheckFailure {
    canister_name: String,
    canister_id: Principal,
    details: String,
}

/// Result of a Candid interface compatibility check.
enum CandidCompatibility {
    /// Both interfaces present and compatible.
    Compatible,
    /// Both interfaces present but the new one is not a subtype of the old.
    Incompatible(String),
    /// Check could not be performed (missing metadata, parse error, etc.).
    Skipped(String),
}

/// Check whether the new WASM module's Candid interface is backward-compatible
/// with the currently deployed one.
///
/// Returns [`CandidCompatibility::Skipped`] if either side lacks a
/// `candid:service` metadata section or if the interfaces cannot be parsed.
async fn check_candid_compatibility(
    agent: &Agent,
    canister_id: &Principal,
    wasm: &[u8],
) -> CandidCompatibility {
    // Extract candid:service from the new WASM module
    let new_candid = match extract_candid_service(wasm) {
        Some(s) => s,
        None => {
            return CandidCompatibility::Skipped(
                "new module does not contain candid:service metadata".into(),
            );
        }
    };

    // Fetch candid:service from the deployed canister
    let old_candid = match fetch_canister_metadata(agent, *canister_id, "candid:service").await {
        Some(s) => s,
        None => {
            return CandidCompatibility::Skipped(
                "deployed canister does not expose candid:service metadata".into(),
            );
        }
    };

    // Parse both interfaces and run the subtype check
    let new_loaded = match CandidSource::Text(&new_candid).load() {
        Ok((env, Some(ty))) => (env, ty),
        Ok((_, None)) => {
            return CandidCompatibility::Skipped(
                "new module candid:service does not define a service".into(),
            );
        }
        Err(e) => {
            return CandidCompatibility::Skipped(format!(
                "failed to parse new module candid:service: {e}"
            ));
        }
    };

    let old_loaded = match CandidSource::Text(&old_candid).load() {
        Ok((env, Some(ty))) => (env, ty),
        Ok((_, None)) => {
            return CandidCompatibility::Skipped(
                "deployed candid:service does not define a service".into(),
            );
        }
        Err(e) => {
            return CandidCompatibility::Skipped(format!(
                "failed to parse deployed candid:service: {e}"
            ));
        }
    };

    let (mut env, new_type) = new_loaded;
    let (env2, old_type) = old_loaded;

    let mut gamma = HashSet::new();
    let old_type = env.merge_type(env2, old_type);
    match subtype_with_config(OptReport::Error, &mut gamma, &env, &new_type, &old_type) {
        Ok(()) => CandidCompatibility::Compatible,
        Err(e) => CandidCompatibility::Incompatible(e.to_string()),
    }
}

pub(crate) async fn install_canister(
    agent: &Agent,
    canister_id: &Principal,
    canister_name: &str,
    wasm: &[u8],
    mode: &str,
    init_args: Option<&[u8]>,
    yes: bool,
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

    // Candid interface compatibility check for upgrades
    if !yes && matches!(install_mode, CanisterInstallMode::Upgrade(_)) {
        match check_candid_compatibility(agent, canister_id, wasm).await {
            CandidCompatibility::Compatible => {}
            CandidCompatibility::Incompatible(details) => {
                return Err(InstallOperationError::CandidIncompatible {
                    canister_name: canister_name.to_owned(),
                    details,
                });
            }
            CandidCompatibility::Skipped(reason) => {
                debug!("Candid compatibility check skipped for {canister_name}: {reason}");
            }
        }
    }

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

/// Installs code to multiple canisters and displays progress bars.
///
/// When `yes` is false, runs a parallel candid compatibility pre-check for all
/// canisters that would be upgraded. If any are incompatible the entire
/// operation is aborted before installing anything.
pub(crate) async fn install_many(
    agent: Agent,
    canisters: Vec<(String, Principal, Option<Vec<u8>>)>,
    mode: &str,
    artifacts: Arc<dyn icp::store_artifact::Access>,
    term: Arc<TermWriter>,
    debug: bool,
    yes: bool,
) -> Result<(), InstallManyError> {
    // Phase 1: Check candid compatibility for all upgrading canisters.
    // Also resolves "auto" mode so the install phase can skip redundant
    // canister_status calls.
    let resolved_modes: Vec<String> = if !yes {
        let mut check_futs = FuturesOrdered::new();
        let check_progress = ProgressManager::new(ProgressManagerSettings { hidden: debug });

        for (name, cid, _) in &canisters {
            let pb = check_progress.create_progress_bar(name);
            let agent = agent.clone();
            let artifacts = artifacts.clone();
            let name = name.clone();
            let cid = *cid;
            let mode = mode.to_string();

            check_futs.push_back(async move {
                pb.set_message("Checking compatibility...");

                // Resolve "auto" into a concrete mode so the install phase
                // can skip the redundant canister_status call.
                let resolved_mode = match mode.as_str() {
                    "auto" => {
                        let mgmt = ManagementCanister::create(&agent);
                        match mgmt.canister_status(&cid).await {
                            Ok((status,)) => {
                                if status.module_hash.is_some() {
                                    "upgrade".to_owned()
                                } else {
                                    "install".to_owned()
                                }
                            }
                            // If we can't query status, pass "auto" through —
                            // install_canister will surface the real error.
                            Err(_) => mode,
                        }
                    }
                    _ => mode,
                };

                let check_result = if resolved_mode == "upgrade" {
                    check_canister_candid_compat(&agent, &cid, &name, &*artifacts)
                        .await
                        .map(|()| resolved_mode)
                } else {
                    Ok(resolved_mode)
                };

                ProgressManager::execute_with_progress(
                    &pb,
                    async { check_result },
                    || "Compatible".to_string(),
                    |_| "Incompatible".to_string(),
                )
                .await
            });
        }

        let mut resolved = Vec::new();
        let mut check_failures: Vec<CandidCheckFailure> = Vec::new();
        while let Some(res) = check_futs.next().await {
            match res {
                Ok(mode) => resolved.push(mode),
                Err(failure) => {
                    resolved.push(String::new());
                    check_failures.push(failure);
                }
            }
        }

        if !check_failures.is_empty() {
            for failure in &check_failures {
                let _ = term.write_line("");
                let _ = term.write_line("");
                let _ = term.write_line(&format!(
                    " ----- Candid interface incompatible: '{}' ({}) -----",
                    failure.canister_name, failure.canister_id,
                ));
                let _ = term.write_line(&format!(
                    "You are making a BREAKING change. Other canisters or frontend clients \
                     relying on your canister may stop working.\n\n{}",
                    failure.details,
                ));
                let _ = term.write_line("");
            }
            let _ = term.write_line("Use --yes to bypass this check.");

            return InstallManySnafu {
                names: check_failures
                    .iter()
                    .map(|f| f.canister_name.clone())
                    .collect::<Vec<_>>(),
            }
            .fail();
        }

        resolved
    } else {
        canisters.iter().map(|_| mode.to_string()).collect()
    };

    // Phase 2: Install all canisters (compatibility already verified or skipped)
    let mut futs = FuturesOrdered::new();
    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: debug });

    for ((name, cid, init_args), resolved_mode) in canisters.into_iter().zip(resolved_modes) {
        let pb = progress_manager.create_progress_bar(&name);
        let agent = agent.clone();
        let install_fn = {
            let pb = pb.clone();
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

                install_canister(
                    &agent,
                    &cid,
                    &name,
                    &wasm,
                    &resolved_mode,
                    init_args.as_deref(),
                    true,
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

            // Map error to include canister context for deferred printing
            result.map_err(|error| InstallFailure {
                canister_name: name.clone(),
                canister_id: cid,
                error,
            })
        });
    }

    // Consume the set of futures and collect errors
    let mut errors: Vec<InstallFailure> = Vec::new();
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
                " ----- Failed to install canister '{}': {} -----",
                failure.canister_name, failure.canister_id,
            ));
            let _ = term.write_line(&format!("Error: '{}'", failure.error));
            let _ = term.write_line("");
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

/// Check candid compatibility for a single canister that is being upgraded.
///
/// Returns `Ok(())` if the check passes or cannot be performed (missing metadata, etc.).
/// Returns `Err(CandidCheckFailure)` only when a genuine incompatibility is found.
async fn check_canister_candid_compat(
    agent: &Agent,
    canister_id: &Principal,
    canister_name: &str,
    artifacts: &dyn icp::store_artifact::Access,
) -> Result<(), CandidCheckFailure> {
    let wasm = match artifacts.lookup(canister_name).await {
        Ok(w) => w,
        // Missing artifact will be caught during install
        Err(_) => return Ok(()),
    };

    match check_candid_compatibility(agent, canister_id, &wasm).await {
        CandidCompatibility::Compatible => Ok(()),
        CandidCompatibility::Skipped(reason) => {
            debug!("Candid compatibility check skipped for {canister_name}: {reason}");
            Ok(())
        }
        CandidCompatibility::Incompatible(details) => Err(CandidCheckFailure {
            canister_name: canister_name.to_owned(),
            canister_id: *canister_id,
            details,
        }),
    }
}
