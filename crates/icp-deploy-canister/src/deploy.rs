//! Canister installation, environment-variable wiring, syncing, and deploy
//! orchestration, expressed entirely over the injected IO traits so the core
//! can run inside a canister.

use std::collections::BTreeMap;

use candid::Principal;
use candid::utils::ArgumentEncoder;
use ic_management_canister_types::{
    CanisterIdRecord, CanisterInstallMode, CanisterSettings, CanisterStatusResult,
    CanisterStatusType, ChunkHash, ClearChunkStoreArgs, EnvironmentVariable,
    InstallChunkedCodeArgs, InstallCodeArgs, UpdateSettingsArgs, UpgradeFlags, UploadChunkArgs,
    WasmMemoryPersistence,
};
use sha2::{Digest, Sha256};
use snafu::prelude::*;

use crate::{
    Canister, Project,
    files::{FileAccess, FileAccessError},
    http::HttpAccess,
    icp_access::{IcpAccess, IcpAccessError},
    ids::IdStore,
    manifest::canister::SyncStep,
    network::Configuration,
    prelude::*,
    sync_exec::{
        PluginExecutor, PluginInvocation, ScriptInvocation, StepProgress, SyncStepContext,
    },
};

/// EOP custom-section metadata key marking a Motoko enhanced-orthogonal-persistence canister.
const EOP_METADATA: &str = "enhanced-orthogonal-persistence";

/// Requested installation mode. `Auto` resolves to `Install` or `Upgrade` by
/// querying the canister's current status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallMode {
    Auto,
    Install,
    Reinstall,
    Upgrade,
}

// ---------------------------------------------------------------------------
// Management-canister transport
// ---------------------------------------------------------------------------

#[derive(Debug, Snafu)]
pub enum MgmtCallError {
    #[snafu(display("failed to encode arguments for management method '{method}'"))]
    Encode {
        method: String,
        source: candid::Error,
    },

    #[snafu(display("management call '{method}' failed"))]
    Call {
        method: String,
        source: IcpAccessError,
    },

    #[snafu(display("failed to decode reply from management method '{method}'"))]
    Decode {
        method: String,
        source: candid::Error,
    },
}

/// Encode + issue a management-canister update call and decode its reply.
///
/// The management canister has no routing of its own, so the effective canister
/// id is the `target`. Candid coding happens here; [`IcpAccess`] is dumb transport.
async fn mgmt_call<A, R>(
    icp: &dyn IcpAccess,
    method: &str,
    target: Principal,
    args: A,
    cycles: u128,
) -> Result<R, MgmtCallError>
where
    A: ArgumentEncoder,
    R: for<'a> candid::utils::ArgumentDecoder<'a>,
{
    let arg = candid::encode_args(args).context(EncodeSnafu { method })?;
    let raw = icp
        .canister_update(
            Principal::management_canister(),
            method,
            arg,
            target,
            cycles,
        )
        .await
        .context(CallSnafu { method })?;
    candid::decode_args(&raw).context(DecodeSnafu { method })
}

// ---------------------------------------------------------------------------
// Install
// ---------------------------------------------------------------------------

#[derive(Debug, Snafu)]
pub enum InstallCanisterError {
    #[snafu(display("failed to read the built artifact for canister '{canister}'"))]
    ReadArtifact {
        canister: String,
        source: FileAccessError,
    },

    #[snafu(display("failed to query status of canister '{canister}'"))]
    CanisterStatus {
        canister: String,
        source: MgmtCallError,
    },

    #[snafu(display("failed to detect orthogonal-persistence metadata on canister '{canister}'"))]
    DetectEop {
        canister: String,
        source: IcpAccessError,
    },

    #[snafu(display("failed to stop canister '{canister}' before upgrade"))]
    StopCanister {
        canister: String,
        source: MgmtCallError,
    },

    #[snafu(display("failed to start canister '{canister}' after upgrade"))]
    StartCanister {
        canister: String,
        source: MgmtCallError,
    },

    #[snafu(display("failed to clear the chunk store for canister '{canister}'"))]
    ClearChunkStore {
        canister: String,
        source: MgmtCallError,
    },

    #[snafu(display("failed to upload wasm chunk {index} for canister '{canister}'"))]
    UploadChunk {
        canister: String,
        index: usize,
        source: MgmtCallError,
    },

    #[snafu(display("failed to install code on canister '{canister}'"))]
    InstallCode {
        canister: String,
        source: MgmtCallError,
    },

    #[snafu(display("failed to install chunked code on canister '{canister}'"))]
    InstallChunkedCode {
        canister: String,
        source: MgmtCallError,
    },
}

/// Query `canister_status` and pick the concrete install mode for `Auto`.
async fn resolve_mode_and_status(
    icp: &dyn IcpAccess,
    canister_name: &str,
    canister_id: Principal,
    mode: InstallMode,
) -> Result<(CanisterInstallMode, CanisterStatusType), InstallCanisterError> {
    let (status,): (CanisterStatusResult,) = mgmt_call(
        icp,
        "canister_status",
        canister_id,
        (CanisterIdRecord { canister_id },),
        0,
    )
    .await
    .context(CanisterStatusSnafu {
        canister: canister_name,
    })?;
    let install_mode = match mode {
        InstallMode::Auto => {
            if status.module_hash.is_some() {
                CanisterInstallMode::Upgrade(None)
            } else {
                CanisterInstallMode::Install
            }
        }
        InstallMode::Install => CanisterInstallMode::Install,
        InstallMode::Reinstall => CanisterInstallMode::Reinstall,
        InstallMode::Upgrade => CanisterInstallMode::Upgrade(None),
    };
    Ok((install_mode, status.status))
}

/// Whether the canister exposes the `enhanced-orthogonal-persistence` metadata.
async fn is_eop_canister(
    icp: &dyn IcpAccess,
    canister_id: Principal,
) -> Result<bool, IcpAccessError> {
    Ok(icp
        .read_canister_metadata(canister_id, EOP_METADATA)
        .await?
        .is_some())
}

/// Install (or upgrade/reinstall) a single, already-built canister.
///
/// Reads the wasm from `artifact_path` through `files`; resolves `Auto` mode and
/// EOP-upgrade flags through `icp`. Large wasm is installed via the chunk store.
#[allow(clippy::too_many_arguments)]
pub async fn install_canister(
    canister_name: &str,
    canister_id: Principal,
    artifact_path: &Path,
    mode: InstallMode,
    init_args: Option<&[u8]>,
    wasm_memory_persistence: Option<WasmMemoryPersistence>,
    files: &dyn FileAccess,
    icp: &dyn IcpAccess,
) -> Result<(), InstallCanisterError> {
    let (mode, status) = resolve_mode_and_status(icp, canister_name, canister_id, mode).await?;
    install_canister_resolved(
        canister_name,
        canister_id,
        artifact_path,
        mode,
        status,
        init_args,
        wasm_memory_persistence,
        files,
        icp,
    )
    .await
}

/// Like [`install_canister`], but with the install mode and current status
/// already resolved by the caller. Callers that need the resolved mode/status
/// for their own logic first (e.g. a Candid-compatibility gate before install)
/// resolve once via [`resolve_install_mode_and_status`] and pass the result here,
/// avoiding a second `canister_status` call.
#[allow(clippy::too_many_arguments)]
pub async fn install_canister_resolved(
    canister_name: &str,
    canister_id: Principal,
    artifact_path: &Path,
    mode: CanisterInstallMode,
    status: CanisterStatusType,
    init_args: Option<&[u8]>,
    wasm_memory_persistence: Option<WasmMemoryPersistence>,
    files: &dyn FileAccess,
    icp: &dyn IcpAccess,
) -> Result<(), InstallCanisterError> {
    let wasm = files
        .read_file(artifact_path)
        .await
        .context(ReadArtifactSnafu {
            canister: canister_name,
        })?;

    // For EOP Motoko canisters an upgrade must set `wasm_memory_persistence`.
    // Trust an explicit caller override; otherwise auto-detect and default to Keep.
    let mode = match mode {
        CanisterInstallMode::Upgrade(_) => {
            let persistence = match wasm_memory_persistence {
                Some(p) => Some(p),
                None => is_eop_canister(icp, canister_id)
                    .await
                    .context(DetectEopSnafu {
                        canister: canister_name,
                    })?
                    .then_some(WasmMemoryPersistence::Keep),
            };
            if let Some(persistence) = persistence {
                CanisterInstallMode::Upgrade(Some(UpgradeFlags {
                    skip_pre_upgrade: None,
                    wasm_memory_persistence: Some(persistence),
                }))
            } else {
                mode
            }
        }
        other => other,
    };

    do_install(
        icp,
        canister_name,
        canister_id,
        &wasm,
        mode,
        status,
        init_args,
    )
    .await
}

/// Resolve an [`InstallMode`] and the canister's current status via
/// `canister_status` (the resolution [`install_canister`] performs internally).
/// Exposed for callers that need the resolved mode before installing.
pub async fn resolve_install_mode_and_status(
    icp: &dyn IcpAccess,
    canister_name: &str,
    canister_id: Principal,
    mode: InstallMode,
) -> Result<(CanisterInstallMode, CanisterStatusType), InstallCanisterError> {
    resolve_mode_and_status(icp, canister_name, canister_id, mode).await
}

#[allow(clippy::too_many_arguments)]
async fn do_install(
    icp: &dyn IcpAccess,
    canister_name: &str,
    canister_id: Principal,
    wasm: &[u8],
    mode: CanisterInstallMode,
    status: CanisterStatusType,
    init_args: Option<&[u8]>,
) -> Result<(), InstallCanisterError> {
    // Raw install_code messages are limited to 2 MiB; larger wasm goes through
    // the chunk store (spec limit is 1 MiB per chunk).
    const CHUNK_THRESHOLD: usize = 2 * 1024 * 1024;
    const CHUNK_SIZE: usize = 1024 * 1024;
    // Generous overhead for encoding, target canister ID, install mode, etc.
    const ENCODING_OVERHEAD: usize = 500;

    let arg = init_args
        .map(|a| a.to_vec())
        .unwrap_or_else(|| candid::encode_args(()).expect("encoding empty args is infallible"));

    let total_install_size = wasm.len() + arg.len() + ENCODING_OVERHEAD;

    if total_install_size <= CHUNK_THRESHOLD {
        let install_args = InstallCodeArgs {
            mode,
            canister_id,
            wasm_module: wasm.to_vec(),
            arg,
            sender_canister_version: None,
        };
        stop_and_start_if_needed(icp, canister_name, canister_id, mode, status, async {
            mgmt_call::<_, ()>(icp, "install_code", canister_id, (install_args,), 0)
                .await
                .context(InstallCodeSnafu {
                    canister: canister_name,
                })
        })
        .await?;
    } else {
        // Clear any existing chunks to ensure a clean state.
        clear_chunk_store(icp, canister_name, canister_id).await?;

        let chunks: Vec<&[u8]> = wasm.chunks(CHUNK_SIZE).collect();
        let mut chunk_hashes: Vec<ChunkHash> = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            let (hash,): (ChunkHash,) = mgmt_call(
                icp,
                "upload_chunk",
                canister_id,
                (UploadChunkArgs {
                    canister_id,
                    chunk: chunk.to_vec(),
                },),
                0,
            )
            .await
            .context(UploadChunkSnafu {
                canister: canister_name,
                index: i,
            })?;
            chunk_hashes.push(hash);
        }

        let wasm_module_hash = Sha256::digest(wasm).to_vec();
        let chunked_args = InstallChunkedCodeArgs {
            mode,
            target_canister: canister_id,
            store_canister: None,
            chunk_hashes_list: chunk_hashes,
            wasm_module_hash,
            arg,
            sender_canister_version: None,
        };

        let install_res =
            stop_and_start_if_needed(icp, canister_name, canister_id, mode, status, async {
                mgmt_call::<_, ()>(icp, "install_chunked_code", canister_id, (chunked_args,), 0)
                    .await
                    .context(InstallChunkedCodeSnafu {
                        canister: canister_name,
                    })
            })
            .await;

        // Always clear the chunk store afterwards to free storage. If the install
        // failed, report that error in preference to a clear-store failure.
        let clear_res = clear_chunk_store(icp, canister_name, canister_id).await;
        install_res?;
        clear_res?;
    }

    Ok(())
}

async fn clear_chunk_store(
    icp: &dyn IcpAccess,
    canister_name: &str,
    canister_id: Principal,
) -> Result<(), InstallCanisterError> {
    mgmt_call::<_, ()>(
        icp,
        "clear_chunk_store",
        canister_id,
        (ClearChunkStoreArgs { canister_id },),
        0,
    )
    .await
    .context(ClearChunkStoreSnafu {
        canister: canister_name,
    })
}

/// Guard an upgrade/reinstall of a Running canister by stopping it first and
/// restarting it afterwards (whether or not the install succeeded).
async fn stop_and_start_if_needed<F>(
    icp: &dyn IcpAccess,
    canister_name: &str,
    canister_id: Principal,
    mode: CanisterInstallMode,
    status: CanisterStatusType,
    install: F,
) -> Result<(), InstallCanisterError>
where
    F: Future<Output = Result<(), InstallCanisterError>>,
{
    let should_guard = matches!(
        mode,
        CanisterInstallMode::Upgrade(_) | CanisterInstallMode::Reinstall
    ) && matches!(status, CanisterStatusType::Running);

    if should_guard {
        mgmt_call::<_, ()>(
            icp,
            "stop_canister",
            canister_id,
            (CanisterIdRecord { canister_id },),
            0,
        )
        .await
        .context(StopCanisterSnafu {
            canister: canister_name,
        })?;
    }

    let install_result = install.await;

    if !should_guard {
        return install_result;
    }

    let start_result = mgmt_call::<_, ()>(
        icp,
        "start_canister",
        canister_id,
        (CanisterIdRecord { canister_id },),
        0,
    )
    .await
    .context(StartCanisterSnafu {
        canister: canister_name,
    });

    // Restart whether or not the install succeeded. If both failed, the install
    // error is the more likely root cause.
    match (install_result, start_result) {
        (Err(install_err), _) => Err(install_err),
        (Ok(()), Err(start_err)) => Err(start_err),
        (Ok(()), Ok(())) => Ok(()),
    }
}

/// Start a canister (idempotent; a no-op if already Running).
pub async fn start_canister(
    icp: &dyn IcpAccess,
    canister_name: &str,
    canister_id: Principal,
) -> Result<(), InstallCanisterError> {
    mgmt_call::<_, ()>(
        icp,
        "start_canister",
        canister_id,
        (CanisterIdRecord { canister_id },),
        0,
    )
    .await
    .context(StartCanisterSnafu {
        canister: canister_name,
    })
}

// ---------------------------------------------------------------------------
// Environment variables + sync
// ---------------------------------------------------------------------------

#[derive(Debug, Snafu)]
pub enum SyncCanisterError {
    #[snafu(display("failed to apply environment variables to canister '{canister}'"))]
    ApplyEnvVars {
        canister: String,
        source: MgmtCallError,
    },

    #[snafu(display("failed to run sync step for canister '{canister}'"))]
    RunStep {
        canister: String,
        source: crate::sync_exec::PluginExecutorError,
    },
}

/// Apply the canister's environment variables — its manifest `settings`
/// variables merged with the generated `PUBLIC_CANISTER_ID:<binding>` variables
/// resolved against `canister_ids` — via `update_settings`.
///
/// This is the piece that standalone `icp sync` previously skipped: the binding
/// ids must be (re)written whenever a canister is synced, not only on deploy.
pub async fn apply_binding_env_vars(
    canister: &Canister,
    canister_id: Principal,
    canister_ids: &BTreeMap<String, Principal>,
    icp: &dyn IcpAccess,
) -> Result<(), SyncCanisterError> {
    let mut env_vars = canister
        .settings
        .environment_variables
        .clone()
        .unwrap_or_default();

    // Each canister is wired only to the ids it declares in `bindings`, resolved
    // to the ids that exist in this environment.
    for (env_name, referenced_key) in &canister.bindings {
        if let Some(principal) = canister_ids.get(referenced_key) {
            env_vars.insert(
                format!("PUBLIC_CANISTER_ID:{env_name}"),
                principal.to_text(),
            );
        }
    }

    let environment_variables: Vec<EnvironmentVariable> = env_vars
        .into_iter()
        .map(|(name, value)| EnvironmentVariable { name, value })
        .collect();

    mgmt_call::<_, ()>(
        icp,
        "update_settings",
        canister_id,
        (UpdateSettingsArgs {
            canister_id,
            settings: CanisterSettings {
                environment_variables: Some(environment_variables),
                ..Default::default()
            },
            sender_canister_version: None,
        },),
        0,
    )
    .await
    .context(ApplyEnvVarsSnafu {
        canister: canister.name.clone(),
    })
}

/// Run a canister's configured sync steps through the injected executor,
/// collecting any retained stderr lines. Does not apply environment variables
/// (see [`apply_binding_env_vars`] / [`sync_canister`]).
pub async fn run_sync_steps(
    canister: &Canister,
    ctx: &SyncStepContext,
    sync_exec: &dyn PluginExecutor,
    progress: Option<&dyn StepProgress>,
) -> Result<Vec<String>, SyncCanisterError> {
    let mut lines = Vec::new();
    for step in &canister.sync.steps {
        // This crate owns dispatch and input derivation; the executor only runs
        // the fully-resolved invocation.
        let step_lines = match step {
            SyncStep::Plugin(adapter) => {
                sync_exec
                    .run_plugin(PluginInvocation::new(adapter, ctx), progress)
                    .await
            }
            SyncStep::Script(adapter) => {
                sync_exec
                    .run_script(ScriptInvocation::new(adapter, ctx), progress)
                    .await
            }
        }
        .context(RunStepSnafu {
            canister: canister.name.clone(),
        })?;
        lines.extend(step_lines);
    }
    Ok(lines)
}

/// Sync a single canister: (re)apply its binding environment variables, then run
/// its sync steps. Applying env vars here is what makes standalone `icp sync`
/// include the generated `PUBLIC_CANISTER_ID:*` variables.
#[allow(clippy::too_many_arguments)]
pub async fn sync_canister(
    canister: &Canister,
    canister_id: Principal,
    ctx: &SyncStepContext,
    icp: &dyn IcpAccess,
    sync_exec: &dyn PluginExecutor,
    progress: Option<&dyn StepProgress>,
) -> Result<Vec<String>, SyncCanisterError> {
    apply_binding_env_vars(canister, canister_id, &ctx.canister_ids, icp).await?;
    run_sync_steps(canister, ctx, sync_exec, progress).await
}

// ---------------------------------------------------------------------------
// Deploy
// ---------------------------------------------------------------------------

#[derive(Debug, Snafu)]
pub enum DeployCanisterError {
    #[snafu(display("could not find canister '{canister}' in environment '{environment}'"))]
    UnknownCanister {
        canister: String,
        environment: String,
    },

    #[snafu(display("could not find an id for canister '{canister}'; create it first"))]
    LookupId {
        canister: String,
        source: crate::ids::IdStoreError,
    },

    #[snafu(display("failed to encode init args for canister '{canister}'"))]
    InitArgs {
        canister: String,
        source: crate::InitArgsToBytesError,
    },

    #[snafu(transparent)]
    Install { source: InstallCanisterError },

    #[snafu(transparent)]
    Sync { source: SyncCanisterError },
}

/// Deploy (install then sync) a single already-built canister in `environment`.
///
/// Assumes the canister already exists (its id is read from `ids`); creating
/// canisters is the caller's responsibility.
#[allow(clippy::too_many_arguments)]
pub async fn deploy_canister(
    project: &Project,
    canister_name: &str,
    environment: &str,
    artifact_path: &Path,
    mode: InstallMode,
    proxy: Option<Principal>,
    files: &dyn FileAccess,
    // Reserved to match the canister-usable API; the current core path does not
    // fetch over HTTP (remote wasm/recipe fetches stay host-side).
    http: &dyn HttpAccess,
    icp: &dyn IcpAccess,
    ids: &dyn IdStore,
    sync_exec: &dyn PluginExecutor,
    progress: Option<&dyn StepProgress>,
) -> Result<Vec<String>, DeployCanisterError> {
    let _ = http;
    let env = project
        .environments
        .get(environment)
        .context(UnknownCanisterSnafu {
            canister: canister_name,
            environment,
        })?;
    let is_cache = matches!(env.network.configuration, Configuration::Managed { .. });
    let network = env.network.name.clone();

    let (canister_path, canister) =
        env.canisters
            .get(canister_name)
            .context(UnknownCanisterSnafu {
                canister: canister_name,
                environment,
            })?;

    let canister_id = ids
        .lookup(is_cache, environment, canister_name)
        .context(LookupIdSnafu {
            canister: canister_name,
        })?;
    let canister_ids = ids
        .lookup_by_environment(is_cache, environment)
        .unwrap_or_default();

    let init_args = canister
        .init_args
        .as_ref()
        .map(|ia| ia.to_bytes())
        .transpose()
        .context(InitArgsSnafu {
            canister: canister_name,
        })?;

    // Environment variables first, then install, then sync steps.
    apply_binding_env_vars(canister, canister_id, &canister_ids, icp).await?;
    install_canister(
        canister_name,
        canister_id,
        artifact_path,
        mode,
        init_args.as_deref(),
        None,
        files,
        icp,
    )
    .await?;
    // Asset sync requires a Running canister; install_code is status-preserving.
    start_canister(icp, canister_name, canister_id).await?;

    let ctx = SyncStepContext {
        canister_path: canister_path.clone(),
        canister_id,
        environment: environment.to_owned(),
        network,
        canister_ids,
        proxy,
    };
    let lines = run_sync_steps(canister, &ctx, sync_exec, progress).await?;
    Ok(lines)
}

#[derive(Debug, Snafu)]
#[snafu(display("failed to deploy canister(s): {}", names.join(", ")))]
pub struct DeployError {
    pub names: Vec<String>,
    pub failures: Vec<(String, DeployCanisterError)>,
}

/// Deploy the `selected` already-built canisters in `environment`, each through
/// [`deploy_canister`]. `artifact_paths` maps canister name → its built wasm
/// path. Per-canister failures are aggregated. The CLI keeps its own fan-out
/// (for progress); this entry point is for programmatic/canister callers.
#[allow(clippy::too_many_arguments)]
pub async fn deploy(
    project: &Project,
    selected: &[String],
    environment: &str,
    mode: InstallMode,
    proxy: Option<Principal>,
    artifact_paths: &BTreeMap<String, PathBuf>,
    files: &dyn FileAccess,
    http: &dyn HttpAccess,
    icp: &dyn IcpAccess,
    ids: &dyn IdStore,
    sync_exec: &dyn PluginExecutor,
) -> Result<(), DeployError> {
    let mut failures = Vec::new();
    for name in selected {
        let Some(artifact_path) = artifact_paths.get(name) else {
            failures.push((
                name.clone(),
                DeployCanisterError::UnknownCanister {
                    canister: name.clone(),
                    environment: environment.to_owned(),
                },
            ));
            continue;
        };
        if let Err(e) = deploy_canister(
            project,
            name,
            environment,
            artifact_path,
            mode,
            proxy,
            files,
            http,
            icp,
            ids,
            sync_exec,
            None,
        )
        .await
        {
            failures.push((name.clone(), e));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        let names = failures.iter().map(|(n, _)| n.clone()).collect();
        Err(DeployError { names, failures })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canister::Settings;
    use crate::manifest::adapter::script::{self, CommandField};
    use crate::manifest::canister::{BuildSteps, SyncStep, SyncSteps};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// Test principal `2vxsx-fae` (the anonymous principal), used as a stand-in.
    fn principal() -> Principal {
        Principal::anonymous()
    }

    /// Records the ordered sequence of interactions across the mock `IcpAccess`
    /// and mock `PluginExecutor`, plus the raw args of each management call.
    #[derive(Default)]
    struct Log {
        events: Vec<String>,
        calls: Vec<(String, Vec<u8>)>,
    }

    struct MockIcp {
        log: Arc<Mutex<Log>>,
    }

    #[cfg_attr(target_family = "wasm", async_trait(?Send))]
    #[cfg_attr(not(target_family = "wasm"), async_trait)]
    impl IcpAccess for MockIcp {
        async fn canister_update(
            &self,
            _canister: Principal,
            method: &str,
            arg: Vec<u8>,
            _effective_canister_id: Principal,
            _cycles: u128,
        ) -> Result<Vec<u8>, IcpAccessError> {
            let mut log = self.log.lock().unwrap();
            log.events.push(method.to_owned());
            log.calls.push((method.to_owned(), arg));
            // Every management method exercised here replies with unit.
            Ok(candid::encode_args(()).unwrap())
        }

        async fn read_canister_metadata(
            &self,
            _canister: Principal,
            _path: &str,
        ) -> Result<Option<Vec<u8>>, IcpAccessError> {
            Ok(None)
        }

        fn caller_principal(&self) -> Principal {
            principal()
        }
    }

    struct MockExec {
        log: Arc<Mutex<Log>>,
    }

    #[cfg_attr(target_family = "wasm", async_trait(?Send))]
    #[cfg_attr(not(target_family = "wasm"), async_trait)]
    impl PluginExecutor for MockExec {
        async fn run_plugin(
            &self,
            _invocation: PluginInvocation,
            _progress: Option<&dyn StepProgress>,
        ) -> Result<Vec<String>, crate::sync_exec::PluginExecutorError> {
            self.log
                .lock()
                .unwrap()
                .events
                .push("run_plugin".to_owned());
            Ok(vec![])
        }

        async fn run_script(
            &self,
            _invocation: ScriptInvocation,
            _progress: Option<&dyn StepProgress>,
        ) -> Result<Vec<String>, crate::sync_exec::PluginExecutorError> {
            self.log
                .lock()
                .unwrap()
                .events
                .push("run_script".to_owned());
            Ok(vec![])
        }
    }

    fn canister_with_binding() -> Canister {
        Canister {
            name: "backend".to_owned(),
            settings: Settings {
                environment_variables: Some(HashMap::from([("FOO".to_owned(), "bar".to_owned())])),
                ..Default::default()
            },
            build: BuildSteps { steps: vec![] },
            sync: SyncSteps {
                steps: vec![SyncStep::Script(script::Adapter {
                    command: CommandField::Command("noop".to_owned()),
                })],
            },
            init_args: None,
            registry_recipe: None,
            bindings: BTreeMap::from([("backend".to_owned(), "backend".to_owned())]),
            friendly_names: vec![],
        }
    }

    fn ctx_for(canister_id: Principal) -> SyncStepContext {
        SyncStepContext {
            canister_path: PathBuf::from("/project"),
            canister_id,
            environment: "local".to_owned(),
            network: "local".to_owned(),
            canister_ids: BTreeMap::from([("backend".to_owned(), canister_id)]),
            proxy: None,
        }
    }

    /// The bug fix: `sync_canister` must (re)apply the binding environment
    /// variables *before* running any sync step, so standalone `icp sync`
    /// includes the generated `PUBLIC_CANISTER_ID:*` variables.
    #[tokio::test]
    async fn sync_canister_applies_env_vars_before_steps() {
        let log = Arc::new(Mutex::new(Log::default()));
        let icp = MockIcp { log: log.clone() };
        let exec = MockExec { log: log.clone() };
        let canister = canister_with_binding();
        let cid = principal();
        let ctx = ctx_for(cid);

        sync_canister(&canister, cid, &ctx, &icp, &exec, None)
            .await
            .unwrap();

        let events = &log.lock().unwrap().events;
        assert_eq!(
            events.as_slice(),
            &["update_settings".to_owned(), "run_script".to_owned()],
            "env vars must be applied before sync steps run"
        );
    }

    /// `apply_binding_env_vars` merges manifest env vars with the generated
    /// `PUBLIC_CANISTER_ID:<binding>` ids.
    #[tokio::test]
    async fn apply_binding_env_vars_merges_manifest_and_bindings() {
        let log = Arc::new(Mutex::new(Log::default()));
        let icp = MockIcp { log: log.clone() };
        let canister = canister_with_binding();
        let cid = principal();
        let canister_ids = BTreeMap::from([("backend".to_owned(), cid)]);

        apply_binding_env_vars(&canister, cid, &canister_ids, &icp)
            .await
            .unwrap();

        let calls = &log.lock().unwrap().calls;
        assert_eq!(calls.len(), 1);
        let (method, arg) = &calls[0];
        assert_eq!(method, "update_settings");
        let (args,): (UpdateSettingsArgs,) = candid::decode_args(arg).unwrap();
        let vars: HashMap<String, String> = args
            .settings
            .environment_variables
            .unwrap()
            .into_iter()
            .map(|v| (v.name, v.value))
            .collect();
        assert_eq!(vars.get("FOO"), Some(&"bar".to_owned()));
        assert_eq!(vars.get("PUBLIC_CANISTER_ID:backend"), Some(&cid.to_text()));
    }
}
