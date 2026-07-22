//! Canister installation, environment-variable wiring, syncing, and deploy
//! orchestration over an `ic-agent` `Agent`, reading built artifacts from disk
//! and running sync plugins in the sandboxed wasmtime engine.

use std::collections::BTreeMap;

use candid::utils::ArgumentEncoder;
use candid::{Encode, Nat, Principal};
use ic_agent::Agent;
use ic_management_canister_types::{
    CanisterIdRecord, CanisterInstallMode, CanisterSettings, CanisterStatusResult,
    CanisterStatusType, ChunkHash, ClearChunkStoreArgs, EnvironmentVariable,
    InstallChunkedCodeArgs, InstallCodeArgs, UpdateSettingsArgs, UpgradeFlags, UploadChunkArgs,
    WasmMemoryPersistence,
};
use icp_canister_interfaces::proxy::{ProxyArgs, ProxyResult};
use sha2::{Digest, Sha256};
use snafu::prelude::*;

use crate::{
    Canister, Project,
    canister::recipe::RemoteResourceResolve,
    ids::IdStore,
    manifest::canister::SyncStep,
    network::Configuration,
    prelude::*,
    sync_exec::{ScriptInvocation, ScriptRunner, StepProgress, SyncStepContext},
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
pub enum UpdateOrProxyError {
    #[snafu(display("failed to encode proxy call arguments"))]
    ProxyEncode { source: candid::Error },

    #[snafu(display("direct update call failed"))]
    DirectUpdateCall { source: ic_agent::AgentError },

    #[snafu(display("proxy update call failed"))]
    ProxyUpdateCall { source: ic_agent::AgentError },

    #[snafu(display("failed to decode proxy canister response"))]
    ProxyDecode { source: candid::Error },

    #[snafu(display("proxy call failed: {message}"))]
    ProxyCall { message: String },
}

/// Dispatch a canister update call, optionally routing through a proxy canister.
///
/// With no proxy, makes a direct call, overriding the effective canister id when
/// given (required for management-canister calls, where it must be the target).
/// With a proxy, wraps the call in [`ProxyArgs`] for the proxy's `proxy` method.
async fn update_or_proxy_raw(
    agent: &Agent,
    canister_id: Principal,
    method: &str,
    arg: Vec<u8>,
    proxy: Option<Principal>,
    effective_canister_id: Option<Principal>,
    cycles: u128,
) -> Result<Vec<u8>, UpdateOrProxyError> {
    if let Some(proxy_cid) = proxy {
        let proxy_args = ProxyArgs {
            canister_id,
            method: method.to_string(),
            args: arg,
            cycles: Nat::from(cycles),
        };
        let proxy_arg_bytes = Encode!(&proxy_args).context(ProxyEncodeSnafu)?;
        let proxy_res = agent
            .update(&proxy_cid, "proxy")
            .with_arg(proxy_arg_bytes)
            .await
            .context(ProxyUpdateCallSnafu)?;
        let proxy_result: (ProxyResult,) =
            candid::decode_args(&proxy_res).context(ProxyDecodeSnafu)?;
        match proxy_result.0 {
            ProxyResult::Ok(ok) => Ok(ok.result),
            ProxyResult::Err(err) => ProxyCallSnafu {
                message: err.format_error(),
            }
            .fail(),
        }
    } else {
        let mut builder = agent.update(&canister_id, method).with_arg(arg);
        if let Some(eid) = effective_canister_id {
            builder = builder.with_effective_canister_id(eid);
        }
        builder.await.context(DirectUpdateCallSnafu)
    }
}

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
        source: UpdateOrProxyError,
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
/// id is the `target`.
async fn mgmt_call<A, R>(
    agent: &Agent,
    proxy: Option<Principal>,
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
    let raw = update_or_proxy_raw(
        agent,
        Principal::management_canister(),
        method,
        arg,
        proxy,
        Some(target),
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
        source: crate::fs::IoError,
    },

    #[snafu(display("failed to query status of canister '{canister}'"))]
    CanisterStatus {
        canister: String,
        source: MgmtCallError,
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
    agent: &Agent,
    proxy: Option<Principal>,
    canister_name: &str,
    canister_id: Principal,
    mode: InstallMode,
) -> Result<(CanisterInstallMode, CanisterStatusType), InstallCanisterError> {
    let (status,): (CanisterStatusResult,) = mgmt_call(
        agent,
        proxy,
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
/// A read failure is treated as "absent", so a missing custom section never
/// aborts an install.
async fn is_eop_canister(agent: &Agent, canister_id: Principal) -> bool {
    agent
        .read_state_canister_metadata(canister_id, EOP_METADATA)
        .await
        .is_ok_and(|section| !section.is_empty())
}

/// Install (or upgrade/reinstall) a single, already-built canister.
///
/// Resolves `Auto` mode and EOP-upgrade flags via `agent`. Large wasm is
/// installed via the chunk store.
#[allow(clippy::too_many_arguments)]
pub async fn install_canister(
    canister_name: &str,
    canister_id: Principal,
    wasm: &[u8],
    mode: InstallMode,
    init_args: Option<&[u8]>,
    wasm_memory_persistence: Option<WasmMemoryPersistence>,
    agent: &Agent,
    proxy: Option<Principal>,
) -> Result<(), InstallCanisterError> {
    let (mode, status) =
        resolve_mode_and_status(agent, proxy, canister_name, canister_id, mode).await?;
    install_canister_resolved(
        canister_name,
        canister_id,
        wasm,
        mode,
        status,
        init_args,
        wasm_memory_persistence,
        agent,
        proxy,
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
    wasm: &[u8],
    mode: CanisterInstallMode,
    status: CanisterStatusType,
    init_args: Option<&[u8]>,
    wasm_memory_persistence: Option<WasmMemoryPersistence>,
    agent: &Agent,
    proxy: Option<Principal>,
) -> Result<(), InstallCanisterError> {
    // For EOP Motoko canisters an upgrade must set `wasm_memory_persistence`.
    // Trust an explicit caller override; otherwise auto-detect and default to Keep.
    let mode = match mode {
        CanisterInstallMode::Upgrade(_) => {
            let persistence = match wasm_memory_persistence {
                Some(p) => Some(p),
                None => is_eop_canister(agent, canister_id)
                    .await
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
        agent,
        proxy,
        canister_name,
        canister_id,
        wasm,
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
    agent: &Agent,
    proxy: Option<Principal>,
    canister_name: &str,
    canister_id: Principal,
    mode: InstallMode,
) -> Result<(CanisterInstallMode, CanisterStatusType), InstallCanisterError> {
    resolve_mode_and_status(agent, proxy, canister_name, canister_id, mode).await
}

#[allow(clippy::too_many_arguments)]
async fn do_install(
    agent: &Agent,
    proxy: Option<Principal>,
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
        stop_and_start_if_needed(
            agent,
            proxy,
            canister_name,
            canister_id,
            mode,
            status,
            async {
                mgmt_call::<_, ()>(
                    agent,
                    proxy,
                    "install_code",
                    canister_id,
                    (install_args,),
                    0,
                )
                .await
                .context(InstallCodeSnafu {
                    canister: canister_name,
                })
            },
        )
        .await?;
    } else {
        // Clear any existing chunks to ensure a clean state.
        clear_chunk_store(agent, proxy, canister_name, canister_id).await?;

        let chunks: Vec<&[u8]> = wasm.chunks(CHUNK_SIZE).collect();
        let mut chunk_hashes: Vec<ChunkHash> = Vec::new();
        for (i, chunk) in chunks.iter().enumerate() {
            let (hash,): (ChunkHash,) = mgmt_call(
                agent,
                proxy,
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

        let install_res = stop_and_start_if_needed(
            agent,
            proxy,
            canister_name,
            canister_id,
            mode,
            status,
            async {
                mgmt_call::<_, ()>(
                    agent,
                    proxy,
                    "install_chunked_code",
                    canister_id,
                    (chunked_args,),
                    0,
                )
                .await
                .context(InstallChunkedCodeSnafu {
                    canister: canister_name,
                })
            },
        )
        .await;

        // Always clear the chunk store afterwards to free storage. If the install
        // failed, report that error in preference to a clear-store failure.
        let clear_res = clear_chunk_store(agent, proxy, canister_name, canister_id).await;
        install_res?;
        clear_res?;
    }

    Ok(())
}

async fn clear_chunk_store(
    agent: &Agent,
    proxy: Option<Principal>,
    canister_name: &str,
    canister_id: Principal,
) -> Result<(), InstallCanisterError> {
    mgmt_call::<_, ()>(
        agent,
        proxy,
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
#[allow(clippy::too_many_arguments)]
async fn stop_and_start_if_needed<F>(
    agent: &Agent,
    proxy: Option<Principal>,
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
            agent,
            proxy,
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
        agent,
        proxy,
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
    agent: &Agent,
    proxy: Option<Principal>,
    canister_name: &str,
    canister_id: Principal,
) -> Result<(), InstallCanisterError> {
    mgmt_call::<_, ()>(
        agent,
        proxy,
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
        source: SyncStepError,
    },
}

#[derive(Debug, Snafu)]
pub enum SyncStepError {
    #[snafu(display("failed to resolve plugin wasm"))]
    ResolveWasm {
        source: crate::canister::recipe::ResolveError,
    },

    #[snafu(display("failed to get identity principal: {err}"))]
    GetIdentityPrincipal { err: String },

    #[snafu(display("plugin run failed"))]
    RunPlugin {
        source: icp_sync_plugin::RunPluginError,
    },

    #[snafu(transparent)]
    Script {
        source: crate::sync_exec::ScriptRunError,
    },
}

/// Compute the environment variables a canister should run with: its manifest
/// `settings` variables merged with the generated `PUBLIC_CANISTER_ID:<binding>`
/// variables, resolved against `canister_ids`.
///
/// Each canister is wired only to the ids it declares in `bindings`, resolved to
/// the ids that exist in this environment; unresolved bindings are skipped.
pub fn binding_env_vars(
    canister: &Canister,
    canister_ids: &BTreeMap<String, Principal>,
) -> Vec<EnvironmentVariable> {
    let mut env_vars = canister
        .settings
        .environment_variables
        .clone()
        .unwrap_or_default();

    for (env_name, referenced_key) in &canister.bindings {
        if let Some(principal) = canister_ids.get(referenced_key) {
            env_vars.insert(
                format!("PUBLIC_CANISTER_ID:{env_name}"),
                principal.to_text(),
            );
        }
    }

    env_vars
        .into_iter()
        .map(|(name, value)| EnvironmentVariable { name, value })
        .collect()
}

/// Apply the canister's environment variables (see [`binding_env_vars`]) via
/// `update_settings`.
///
/// This is the piece that standalone `icp sync` previously skipped: the binding
/// ids must be (re)written whenever a canister is synced, not only on deploy.
pub async fn apply_binding_env_vars(
    canister: &Canister,
    canister_id: Principal,
    canister_ids: &BTreeMap<String, Principal>,
    agent: &Agent,
    proxy: Option<Principal>,
) -> Result<(), SyncCanisterError> {
    let environment_variables = binding_env_vars(canister, canister_ids);

    mgmt_call::<_, ()>(
        agent,
        proxy,
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

/// Resolve and run one plugin sync step in the sandboxed wasmtime engine. The
/// wasm source is resolved (fetched/verified/cached) through `resolver`.
async fn run_plugin_step(
    adapter: &crate::manifest::adapter::plugin::Adapter,
    ctx: &SyncStepContext,
    agent: &Agent,
    resolver: &dyn RemoteResourceResolve,
    stdio: Option<tokio::sync::mpsc::Sender<String>>,
) -> Result<Vec<String>, SyncStepError> {
    let wasm_path = resolver
        .resolve_wasm(
            &adapter.source,
            &ctx.canister_path,
            adapter.sha256.as_deref(),
            stdio.clone(),
        )
        .await
        .context(ResolveWasmSnafu)?;

    let identity_principal = agent
        .get_principal()
        .map_err(|err| SyncStepError::GetIdentityPrincipal { err })?;
    let dirs = adapter.dirs.clone().unwrap_or_default();
    let files = adapter.files.clone().unwrap_or_default();
    let agent = agent.clone();
    let base_dir = ctx.canister_path.clone();
    let cid = ctx.canister_id;
    let proxy = ctx.proxy;
    let environment = ctx.environment.clone();

    // Blocking wasmtime call — signal Tokio that this thread will block.
    tokio::task::block_in_place(|| {
        icp_sync_plugin::run_plugin(
            wasm_path,
            base_dir,
            dirs,
            files,
            cid,
            agent,
            proxy,
            identity_principal,
            environment,
            stdio,
        )
    })
    .context(RunPluginSnafu)
}

/// Run a canister's configured sync steps, collecting any retained stderr lines.
/// Plugin steps run in the sandboxed wasmtime engine (their wasm resolved via
/// `resolver`); script steps run through `script_runner`. Does not apply
/// environment variables (see [`apply_binding_env_vars`] / [`sync_canister`]).
pub async fn run_sync_steps(
    canister: &Canister,
    ctx: &SyncStepContext,
    agent: &Agent,
    resolver: &dyn RemoteResourceResolve,
    script_runner: &dyn ScriptRunner,
    mut progress: Option<&mut dyn StepProgress>,
) -> Result<Vec<String>, SyncCanisterError> {
    let total = canister.sync.steps.len();
    let mut lines = Vec::new();
    for (i, step) in canister.sync.steps.iter().enumerate() {
        let header = format!("\nSyncing: {step} {} of {total}", i + 1);
        let stdio = progress.as_deref_mut().and_then(|p| p.begin_step(header));
        let step_lines = match step {
            SyncStep::Plugin(adapter) => {
                run_plugin_step(adapter, ctx, agent, resolver, stdio).await
            }
            SyncStep::Script(adapter) => script_runner
                .run_script(ScriptInvocation::new(adapter, ctx), stdio)
                .await
                .map_err(|source| SyncStepError::Script { source }),
        };
        if let Some(p) = progress.as_deref_mut() {
            p.end_step().await;
        }
        lines.extend(step_lines.context(RunStepSnafu {
            canister: canister.name.clone(),
        })?);
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
    agent: &Agent,
    resolver: &dyn RemoteResourceResolve,
    script_runner: &dyn ScriptRunner,
    progress: Option<&mut dyn StepProgress>,
) -> Result<Vec<String>, SyncCanisterError> {
    apply_binding_env_vars(canister, canister_id, &ctx.canister_ids, agent, ctx.proxy).await?;
    run_sync_steps(canister, ctx, agent, resolver, script_runner, progress).await
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
    agent: &Agent,
    ids: &dyn IdStore,
    resolver: &dyn RemoteResourceResolve,
    script_runner: &dyn ScriptRunner,
    progress: Option<&mut dyn StepProgress>,
) -> Result<Vec<String>, DeployCanisterError> {
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

    let wasm = crate::fs::read(artifact_path).context(ReadArtifactSnafu {
        canister: canister_name,
    })?;

    // Environment variables first, then install, then sync steps.
    apply_binding_env_vars(canister, canister_id, &canister_ids, agent, proxy).await?;
    install_canister(
        canister_name,
        canister_id,
        &wasm,
        mode,
        init_args.as_deref(),
        None,
        agent,
        proxy,
    )
    .await?;
    // Asset sync requires a Running canister; install_code is status-preserving.
    start_canister(agent, proxy, canister_name, canister_id).await?;

    let ctx = SyncStepContext {
        canister_path: canister_path.clone(),
        canister_id,
        environment: environment.to_owned(),
        network,
        canister_ids,
        proxy,
    };
    let lines = run_sync_steps(canister, &ctx, agent, resolver, script_runner, progress).await?;
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
    agent: &Agent,
    ids: &dyn IdStore,
    resolver: &dyn RemoteResourceResolve,
    script_runner: &dyn ScriptRunner,
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
            agent,
            ids,
            resolver,
            script_runner,
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
    use crate::manifest::canister::{BuildSteps, SyncSteps};
    use std::collections::HashMap;

    fn canister(env_vars: &[(&str, &str)], bindings: &[(&str, &str)]) -> Canister {
        Canister {
            name: "backend".to_owned(),
            settings: Settings {
                environment_variables: Some(
                    env_vars
                        .iter()
                        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                        .collect::<HashMap<_, _>>(),
                ),
                ..Default::default()
            },
            build: BuildSteps { steps: vec![] },
            sync: SyncSteps { steps: vec![] },
            init_args: None,
            registry_recipe: None,
            bindings: bindings
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            friendly_names: vec![],
        }
    }

    fn vars(canister: &Canister, ids: &[(&str, Principal)]) -> HashMap<String, String> {
        let ids: BTreeMap<String, Principal> =
            ids.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect();
        binding_env_vars(canister, &ids)
            .into_iter()
            .map(|v| (v.name, v.value))
            .collect()
    }

    /// Manifest env vars and resolved `PUBLIC_CANISTER_ID:<binding>` ids are both
    /// present in the computed environment.
    #[test]
    fn merges_manifest_vars_and_resolved_bindings() {
        let cid = Principal::anonymous();
        let c = canister(&[("FOO", "bar")], &[("frontend", "frontend")]);
        let v = vars(&c, &[("frontend", cid)]);

        assert_eq!(v.get("FOO").map(String::as_str), Some("bar"));
        assert_eq!(
            v.get("PUBLIC_CANISTER_ID:frontend").map(String::as_str),
            Some(cid.to_text().as_str())
        );
    }

    /// A binding whose referenced canister has no id in this environment is
    /// skipped rather than emitted with an empty value.
    #[test]
    fn unresolved_binding_is_skipped() {
        let c = canister(&[], &[("frontend", "frontend")]);
        let v = vars(&c, &[]);
        assert!(v.is_empty(), "expected no vars, got {v:?}");
    }
}
