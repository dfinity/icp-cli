use futures::{StreamExt, stream::FuturesOrdered};
use ic_agent::{Agent, export::Principal};
use ic_management_canister_types::{
    CanisterId, CanisterIdRecord, CanisterInstallMode, CanisterStatusType, WasmMemoryPersistence,
};
use icp_deploy_canister::{InstallCanisterError, install_canister_resolved, install_canister_wasm};
use icp_events::TaskOutcome;
use snafu::{ResultExt, Snafu};
use std::sync::Arc;

use crate::operations::access::{AgentIcpAccess, ArtifactFileAccess};
use crate::operations::task::{Reporter, Task};
use crate::prelude::*;

use super::misc::fetch_canister_metadata;
use super::proxy::UpdateOrProxyError;
use super::proxy_management;

/// CLI-facing choice for `wasm_memory_persistence` on EOP upgrades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
pub enum WasmMemoryPersistenceOpt {
    /// Preserve canister main memory across upgrade (normal EOP upgrade).
    Keep,
    /// Discard canister main memory; only `stable` variables survive.
    /// Dangerous — heap state is lost.
    Replace,
}

impl WasmMemoryPersistenceOpt {
    fn to_ic(self) -> WasmMemoryPersistence {
        match self {
            WasmMemoryPersistenceOpt::Keep => WasmMemoryPersistence::Keep,
            WasmMemoryPersistenceOpt::Replace => WasmMemoryPersistence::Replace,
        }
    }
}

/// Returns true if the canister exposes the `enhanced-orthogonal-persistence`
/// custom-section metadata (i.e. it is a Motoko EOP canister).
pub async fn is_eop_canister(agent: &Agent, canister_id: &Principal) -> bool {
    fetch_canister_metadata(agent, *canister_id, "enhanced-orthogonal-persistence")
        .await
        .is_some()
}

#[derive(Debug, Snafu)]
#[snafu(display("Canister(s) {names:?} failed to install."))]
pub struct InstallManyError {
    names: Vec<String>,
}

/// Resolve a mode string ("auto", "install", "reinstall", "upgrade") into
/// a [`CanisterInstallMode`]. For "auto", queries `canister_status` to
/// determine whether the canister already has code installed.
///
/// Returns the resolved mode plus the current status; callers (deploy, the
/// candid-compat gate) need the resolved mode before installing, so resolution
/// happens here once and the result is handed to [`install_canister`] or
/// [`install_stored_canister`].
pub async fn resolve_install_mode_and_status(
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
pub struct ResolveInstallModeError {
    canister_name: String,
    source: UpdateOrProxyError,
}

/// Install a module a caller already holds in memory. The
/// install-code/chunking/EOP logic lives in
/// `icp_deploy_canister::install_canister_wasm`; this is a thin wrapper over the
/// agent-backed `IcpAccess`.
#[allow(clippy::too_many_arguments)]
pub async fn install_canister(
    agent: &Agent,
    proxy: Option<Principal>,
    canister_id: &Principal,
    canister_name: &str,
    wasm: &[u8],
    mode: CanisterInstallMode,
    status: CanisterStatusType,
    init_args: Option<&[u8]>,
    wasm_memory_persistence: Option<WasmMemoryPersistenceOpt>,
) -> Result<(), InstallCanisterError> {
    let icp = AgentIcpAccess::new(agent.clone(), proxy);
    install_canister_wasm(
        canister_name,
        *canister_id,
        wasm,
        mode,
        status,
        init_args,
        wasm_memory_persistence.map(WasmMemoryPersistenceOpt::to_ic),
        &icp,
    )
    .await
}

/// Install one canister whose build artifact lives in the store, addressed by
/// its store key `canister_name`. Like [`install_canister`], but reading the
/// module through the artifact-backed `FileAccess` rather than taking its bytes.
#[allow(clippy::too_many_arguments)]
async fn install_stored_canister(
    icp: &AgentIcpAccess,
    files: &ArtifactFileAccess,
    canister_id: &Principal,
    canister_name: &str,
    mode: CanisterInstallMode,
    status: CanisterStatusType,
    init_args: Option<&[u8]>,
    wasm_memory_persistence: Option<WasmMemoryPersistenceOpt>,
) -> Result<(), InstallCanisterError> {
    install_canister_resolved(
        canister_name,
        *canister_id,
        // The artifact `FileAccess` resolves the store key, so the "path" is the
        // canister name.
        Path::new(canister_name),
        mode,
        status,
        init_args,
        wasm_memory_persistence.map(WasmMemoryPersistenceOpt::to_ic),
        files,
        icp,
    )
    .await
}

/// Installs code to multiple canisters concurrently.
pub async fn install_many(
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
    artifacts: Arc<dyn crate::store_artifact::Access>,
    reporter: &Reporter,
) -> Result<(), InstallManyError> {
    let icp = Arc::new(AgentIcpAccess::new(agent, proxy));
    let files = Arc::new(ArtifactFileAccess(artifacts));

    let mut futs = FuturesOrdered::new();

    for (name, cid, mode, status, init_args) in canisters {
        let task = reporter.task(Task::install(name.clone(), cid));
        let icp = icp.clone();
        let files = files.clone();

        futs.push_back(async move {
            let result = install_stored_canister(
                &icp,
                &files,
                &cid,
                &name,
                mode,
                status,
                init_args.as_deref(),
                None,
            )
            .await;

            match &result {
                Ok(()) => task.finish(TaskOutcome::succeeded()),
                Err(error) => task.finish(TaskOutcome::failed(error.to_string())),
            }

            result.map_err(|_| name)
        });
    }

    // Collect the failed canister names; the renderer owns displaying each
    // failure.
    let mut failed: Vec<String> = Vec::new();
    while let Some(res) = futs.next().await {
        if let Err(name) = res {
            failed.push(name);
        }
    }

    if !failed.is_empty() {
        return InstallManySnafu { names: failed }.fail();
    }

    Ok(())
}
