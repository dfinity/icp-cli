//! Programmatic canister deployment: create canisters and install wasm through
//! the management canister, without shelling out to the `icp` binary.
//!
//! This is a **minimal, self-contained public surface** intended for external
//! consumers (for example a backend service deploying prebuilt marketplace app
//! bundles) that need to create canisters and install code from Rust.
//!
//! Scope: it performs **direct** management-canister `create_canister` calls
//! with **no cycles attached**. That only works on subnets that permit
//! cycles-free creation — i.e. CloudEngine-style engine subnets, which are the
//! intended consumer. It is **not** a general local-replica / cycles-ledger /
//! cycles-minting-canister flow: subnets that require cycles-ledger or CMC
//! funding to create a canister are out of scope for this API. Use the `icp`
//! binary's `operations::create` (cycles ledger / CMC / proxy funding) for
//! those.
//!
//! The `icp` binary's own `operations::{create,install}` layer could converge
//! onto these functions later; today it carries extra machinery (progress bars,
//! proxy routing, EOP auto-detection, ICP/CMC funding) that a library consumer
//! does not need.

use candid::{Decode, Encode, Principal};
use ic_agent::{Agent, AgentError};
use ic_management_canister_types::{
    CanisterId, CanisterIdRecord, CanisterInstallMode, CanisterSettings, CanisterStatusResult,
    ChunkHash, ClearChunkStoreArgs, CreateCanisterArgs, InstallChunkedCodeArgs, InstallCodeArgs,
    UploadChunkArgs,
};
use sha2::{Digest, Sha256};
use snafu::{OptionExt, ResultExt, Snafu};

/// Raw `install_code` messages are capped at 2 MiB; anything larger must go
/// through the chunked-install flow.
const CHUNK_THRESHOLD: usize = 2 * 1024 * 1024;
/// Per-chunk size for chunked installs (spec limit is 1 MiB per chunk).
const CHUNK_SIZE: usize = 1024 * 1024;
/// Head-room for candid encoding, the target id, install mode, etc. when
/// deciding whether the install message fits under [`CHUNK_THRESHOLD`].
const ENCODING_OVERHEAD: usize = 500;

/// Decide whether an install carrying `wasm_len` bytes of module and `arg_len`
/// bytes of init args must use the chunked-install flow: `true` when the encoded
/// `install_code` message would exceed the 2 MiB [`CHUNK_THRESHOLD`], `false`
/// when it fits in a single message. Pure so the boundary is unit-testable.
fn needs_chunked_install(wasm_len: usize, arg_len: usize) -> bool {
    wasm_len + arg_len + ENCODING_OVERHEAD > CHUNK_THRESHOLD
}

#[derive(Debug, Snafu)]
pub enum DeployError {
    #[snafu(display("failed to encode candid arguments"))]
    Encode { source: candid::Error },

    #[snafu(display("failed to decode candid response"))]
    Decode { source: candid::Error },

    #[snafu(display("management canister call failed"))]
    Agent { source: AgentError },

    #[snafu(display(
        "subnet {subnet} exposes no canister-id ranges to derive an effective canister id from"
    ))]
    NoCanisterRanges { subnet: Principal },
}

/// Derive an effective canister id for a management-canister `create_canister`
/// call targeting `subnet`: the first principal in the subnet's canister-id
/// ranges. `create_canister` has no natural target canister of its own, so the
/// agent needs an effective id that routes the request to the intended subnet.
///
/// This is meant for the direct, cycles-free creation flow against
/// CloudEngine-style engine subnets (see the [module docs](self)); it is not a
/// cycles-ledger / CMC creation helper.
pub async fn effective_canister_id_for_subnet(
    agent: &Agent,
    subnet: Principal,
) -> Result<Principal, DeployError> {
    let subnet_info = agent.get_subnet_by_id(&subnet).await.context(AgentSnafu)?;
    let start = *subnet_info
        .iter_canister_ranges()
        .next()
        .context(NoCanisterRangesSnafu { subnet })?
        .start();
    Ok(start)
}

/// Create a canister via a **direct** management-canister `create_canister`
/// call, routing the request to the subnet that owns `effective_canister_id`
/// (any principal within the target subnet's id range — see
/// [`effective_canister_id_for_subnet`]). `settings` carries the controllers,
/// compute/memory allocation, etc.
///
/// **No cycles are attached.** This only succeeds on subnets that permit
/// cycles-free creation — CloudEngine-style engine subnets, the intended
/// consumer of this API. Subnets that require cycles-ledger or CMC funding
/// (e.g. mainnet) are out of scope here; use the `icp` binary's
/// `operations::create` for those.
pub async fn create_canister(
    agent: &Agent,
    effective_canister_id: Principal,
    settings: CanisterSettings,
) -> Result<Principal, DeployError> {
    let arg = CreateCanisterArgs {
        settings: Some(settings),
        sender_canister_version: None,
    };
    let resp = agent
        .update(&Principal::management_canister(), "create_canister")
        .with_arg(Encode!(&arg).context(EncodeSnafu)?)
        .with_effective_canister_id(effective_canister_id)
        .call_and_wait()
        .await
        .context(AgentSnafu)?;
    let record = Decode!(&resp, CanisterIdRecord).context(DecodeSnafu)?;
    Ok(record.canister_id)
}

/// Convenience wrapper over [`create_canister`] that resolves the effective
/// canister id from `subnet` for you.
///
/// Same scope as [`create_canister`]: direct, cycles-free creation suitable for
/// CloudEngine-style engine subnets only. Subnets requiring cycles-ledger / CMC
/// funding are out of scope — use the `icp` binary's `operations::create`.
pub async fn create_canister_on_subnet(
    agent: &Agent,
    subnet: Principal,
    settings: CanisterSettings,
) -> Result<Principal, DeployError> {
    let effective = effective_canister_id_for_subnet(agent, subnet).await?;
    create_canister(agent, effective, settings).await
}

/// Query the canister's status through the management canister.
pub async fn canister_status(
    agent: &Agent,
    canister_id: Principal,
) -> Result<CanisterStatusResult, DeployError> {
    let arg = CanisterIdRecord {
        canister_id: CanisterId::from(canister_id),
    };
    let resp = agent
        .update(&Principal::management_canister(), "canister_status")
        .with_arg(Encode!(&arg).context(EncodeSnafu)?)
        .with_effective_canister_id(canister_id)
        .call_and_wait()
        .await
        .context(AgentSnafu)?;
    Decode!(&resp, CanisterStatusResult).context(DecodeSnafu)
}

/// Resolve the install mode the way `icp deploy --mode auto` does: `Upgrade` if
/// the canister already has a module installed, otherwise `Install`.
pub async fn resolve_install_mode(
    agent: &Agent,
    canister_id: Principal,
) -> Result<CanisterInstallMode, DeployError> {
    let status = canister_status(agent, canister_id).await?;
    Ok(if status.module_hash.is_some() {
        CanisterInstallMode::Upgrade(None)
    } else {
        CanisterInstallMode::Install
    })
}

/// Install `wasm` into `canister_id` via the management canister, transparently
/// switching to the chunked-install flow for modules that exceed the 2 MiB
/// message limit. `init_args` are the candid-encoded arguments passed to
/// `canister_init` / `canister_post_upgrade` (`None` encodes empty args).
///
/// This is deliberately a plain `install_code`; it does not stop/start the
/// canister around an upgrade or auto-detect enhanced-orthogonal-persistence.
/// Callers that need those guarantees should stop the canister first (see the
/// binary's `operations::install` for the full behavior).
pub async fn install_wasm(
    agent: &Agent,
    canister_id: Principal,
    wasm: &[u8],
    mode: CanisterInstallMode,
    init_args: Option<&[u8]>,
) -> Result<(), DeployError> {
    let cid = CanisterId::from(canister_id);
    let arg = init_args
        .map(|a| a.to_vec())
        .unwrap_or_else(|| Encode!().expect("encoding empty candid args cannot fail"));

    if !needs_chunked_install(wasm.len(), arg.len()) {
        let install_args = InstallCodeArgs {
            mode,
            canister_id: cid,
            wasm_module: wasm.to_vec(),
            arg,
            sender_canister_version: None,
        };
        mgmt_call(agent, canister_id, "install_code", &install_args).await?;
        return Ok(());
    }

    // Large module: clear any stale chunks, upload the wasm in chunks, then
    // install by hash. Anything from here on can leave chunks charged to the
    // canister, so the chunk store is cleared again on *every* outcome below —
    // not just success — while the original error (if any) is preserved.
    clear_chunk_store(agent, canister_id, cid).await?;

    let install_result = upload_and_install_chunked(agent, canister_id, cid, wasm, mode, arg).await;

    // Free the chunk store regardless of the outcome. A cleanup failure must not
    // mask an upload/install error, so on `Err` we drop the (best-effort) clear
    // result and return the original error; on `Ok` we surface a clear failure.
    let clear_result = clear_chunk_store(agent, canister_id, cid).await;
    match install_result {
        Ok(()) => clear_result,
        Err(e) => Err(e),
    }
}

/// Upload `wasm` to `canister_id`'s chunk store in [`CHUNK_SIZE`] pieces, then
/// `install_chunked_code` by hash. Split out of [`install_wasm`] so the caller
/// can run chunk-store cleanup around it on every outcome.
async fn upload_and_install_chunked(
    agent: &Agent,
    canister_id: Principal,
    cid: CanisterId,
    wasm: &[u8],
    mode: CanisterInstallMode,
    arg: Vec<u8>,
) -> Result<(), DeployError> {
    let mut chunk_hashes: Vec<ChunkHash> = Vec::new();
    for chunk in wasm.chunks(CHUNK_SIZE) {
        let upload_args = UploadChunkArgs {
            canister_id: cid,
            chunk: chunk.to_vec(),
        };
        let resp = agent
            .update(&Principal::management_canister(), "upload_chunk")
            .with_arg(Encode!(&upload_args).context(EncodeSnafu)?)
            .with_effective_canister_id(canister_id)
            .call_and_wait()
            .await
            .context(AgentSnafu)?;
        chunk_hashes.push(Decode!(&resp, ChunkHash).context(DecodeSnafu)?);
    }

    let wasm_module_hash = Sha256::digest(wasm).to_vec();
    let chunked_args = InstallChunkedCodeArgs {
        mode,
        target_canister: cid,
        store_canister: None,
        chunk_hashes_list: chunk_hashes,
        wasm_module_hash,
        arg,
        sender_canister_version: None,
    };
    mgmt_call(agent, canister_id, "install_chunked_code", &chunked_args).await
}

/// Encode `arg`, send it as an update to the management canister for `method`,
/// and discard the (unit) reply. Used for calls whose response we do not need.
async fn mgmt_call<T: candid::CandidType>(
    agent: &Agent,
    canister_id: Principal,
    method: &str,
    arg: &T,
) -> Result<(), DeployError> {
    agent
        .update(&Principal::management_canister(), method)
        .with_arg(Encode!(arg).context(EncodeSnafu)?)
        .with_effective_canister_id(canister_id)
        .call_and_wait()
        .await
        .context(AgentSnafu)?;
    Ok(())
}

async fn clear_chunk_store(
    agent: &Agent,
    canister_id: Principal,
    cid: CanisterId,
) -> Result<(), DeployError> {
    mgmt_call(
        agent,
        canister_id,
        "clear_chunk_store",
        &ClearChunkStoreArgs { canister_id: cid },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    // The install flow itself (upload_chunk / install_code / clear_chunk_store
    // ordering and the failure-cleanup / error-precedence path in `install_wasm`)
    // talks to the management canister through `ic_agent::Agent`, which has no
    // lightweight fake to inject here, so that path needs a live-replica
    // integration test rather than a unit test. What is unit-testable without a
    // network is the pure chunking-threshold decision, covered below.

    #[test]
    fn small_install_fits_single_message() {
        assert!(!needs_chunked_install(0, 0));
        assert!(!needs_chunked_install(1024, 0));
        assert!(!needs_chunked_install(1024, 1024));
    }

    #[test]
    fn threshold_boundary_is_inclusive() {
        // wasm + arg + overhead == CHUNK_THRESHOLD still fits in one message.
        let wasm_at_limit = CHUNK_THRESHOLD - ENCODING_OVERHEAD;
        assert!(!needs_chunked_install(wasm_at_limit, 0));
        // One byte over the limit tips into the chunked flow.
        assert!(needs_chunked_install(wasm_at_limit + 1, 0));
    }

    #[test]
    fn init_args_can_push_over_the_threshold() {
        let wasm_len = CHUNK_THRESHOLD - ENCODING_OVERHEAD - 10;
        // Wasm alone fits...
        assert!(!needs_chunked_install(wasm_len, 0));
        // ...but adding enough init-arg bytes to exceed the limit forces chunking.
        assert!(needs_chunked_install(wasm_len, 11));
    }

    #[test]
    fn large_module_requires_chunking() {
        assert!(needs_chunked_install(8 * 1024 * 1024, 0));
    }

    #[test]
    fn chunk_size_is_within_the_spec_per_chunk_limit() {
        // Each uploaded chunk must stay under the 1 MiB per-chunk spec limit, and
        // a module just over the single-message threshold must split into >1 chunk.
        const { assert!(CHUNK_SIZE <= 1024 * 1024) };
        let wasm = vec![0u8; CHUNK_THRESHOLD + 1];
        let chunks = wasm.chunks(CHUNK_SIZE).count();
        assert!(chunks > 1);
        assert!(wasm.chunks(CHUNK_SIZE).all(|c| c.len() <= CHUNK_SIZE));
    }
}
