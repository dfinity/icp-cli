use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, export::Principal};
use ic_management_canister_types::{
    CanisterId, CanisterIdRecord, CanisterInstallMode, CanisterStatusType, WasmMemoryPersistence,
};
use icp_deploy_canister::{InstallCanisterError, install_canister_resolved};
use snafu::{ResultExt, Snafu};
use std::sync::Arc;
use tracing::error;

use crate::progress::{ProgressManager, ProgressManagerSettings};

use super::misc::fetch_canister_metadata;
use super::proxy::UpdateOrProxyError;
use super::proxy_management;

/// CLI-facing choice for `wasm_memory_persistence` on EOP upgrades.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum WasmMemoryPersistenceOpt {
    /// Preserve canister main memory across upgrade (normal EOP upgrade).
    Keep,
    /// Discard canister main memory; only `stable` variables survive.
    /// Dangerous — heap state is lost.
    Replace,
}

impl WasmMemoryPersistenceOpt {
    pub(crate) fn to_ic(self) -> WasmMemoryPersistence {
        match self {
            WasmMemoryPersistenceOpt::Keep => WasmMemoryPersistence::Keep,
            WasmMemoryPersistenceOpt::Replace => WasmMemoryPersistence::Replace,
        }
    }
}

/// Returns true if the canister exposes the `enhanced-orthogonal-persistence`
/// custom-section metadata (i.e. it is a Motoko EOP canister).
pub(crate) async fn is_eop_canister(agent: &Agent, canister_id: &Principal) -> bool {
    fetch_canister_metadata(agent, *canister_id, "enhanced-orthogonal-persistence")
        .await
        .is_some()
}

/// Resolve a mode string ("auto", "install", "reinstall", "upgrade") into
/// a [`CanisterInstallMode`]. For "auto", queries `canister_status` to
/// determine whether the canister already has code installed.
///
/// Returns the resolved mode plus the current status; callers (deploy, the
/// candid-compat gate) need the resolved mode before installing, so resolution
/// happens here once and the result is handed to [`install_canister_resolved`].
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

#[derive(Debug, Snafu)]
pub(crate) enum InstallStoredError {
    #[snafu(display("failed to read the built artifact for canister '{canister}'"))]
    ReadArtifact {
        canister: String,
        source: icp::store_artifact::LookupArtifactError,
    },

    #[snafu(transparent)]
    Install { source: InstallCanisterError },
}

/// Install one canister whose build artifact lives in the store, addressed by
/// its store key `canister_name`. The install-code/chunking/EOP logic lives in
/// `icp_deploy_canister::install_canister_resolved`; this reads the built wasm
/// from the artifact store and hands it to the agent-backed installer.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn install_stored_canister(
    agent: &Agent,
    proxy: Option<Principal>,
    artifacts: &dyn icp::store_artifact::Access,
    canister_id: &Principal,
    canister_name: &str,
    mode: CanisterInstallMode,
    status: CanisterStatusType,
    init_args: Option<&[u8]>,
    wasm_memory_persistence: Option<WasmMemoryPersistenceOpt>,
) -> Result<(), InstallStoredError> {
    let wasm = artifacts
        .lookup(canister_name)
        .await
        .context(ReadArtifactSnafu {
            canister: canister_name,
        })?;
    install_canister_resolved(
        canister_name,
        *canister_id,
        &wasm,
        mode,
        status,
        init_args,
        wasm_memory_persistence.map(WasmMemoryPersistenceOpt::to_ic),
        agent,
        proxy,
    )
    .await?;
    Ok(())
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
    error: InstallStoredError,
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
        let artifacts = artifacts.clone();
        let install_fn = {
            let pb = pb.clone();
            let name = name.clone();

            async move {
                pb.set_message("Installing...");
                install_stored_canister(
                    &agent,
                    proxy,
                    artifacts.as_ref(),
                    &cid,
                    &name,
                    mode,
                    status,
                    init_args.as_deref(),
                    None,
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
