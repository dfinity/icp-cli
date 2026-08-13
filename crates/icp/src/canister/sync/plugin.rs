use camino::Utf8PathBuf;
use ic_agent::Agent;
use icp_deploy_canister::canister::recipe::{RemoteResourceResolve, ResolveError};
use icp_deploy_canister::sync_exec::PluginInvocation;
use icp_sync_plugin::{RunPluginError, run_plugin};
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::canister::ChannelProgress;

#[derive(Debug, Snafu)]
pub enum PluginError {
    #[snafu(display("failed to resolve plugin wasm"))]
    ResolveWasm { source: ResolveError },

    #[snafu(display("failed to get identity principal: {err}"))]
    GetIdentityPrincipal { err: String },

    #[snafu(display("failed to run plugin"))]
    Run { source: RunPluginError },
}

/// Fetch and run a WASI plugin against a canister for a fully-resolved
/// [`PluginInvocation`]. Dispatch and input derivation happen in
/// `icp-deploy-canister`; this only performs the host-only wasm resolution and
/// wasmtime execution.
pub(super) async fn run(
    invocation: &PluginInvocation,
    agent: &Agent,
    stdio: Option<Sender<String>>,
    resolver: &dyn RemoteResourceResolve,
) -> Result<Vec<String>, PluginError> {
    // 1. Determine the on-disk path for the wasm. run_plugin needs a path, not raw bytes.
    //    - Local: sha256 is verified if present, then the original path is returned.
    //    - Remote: downloaded to cache (sha256 required, enforced at parse time) and the
    //      stable cache path is returned — no temp file needed.
    let progress = ChannelProgress::wrap(stdio.as_ref());
    let wasm_path = resolver
        .resolve_wasm(
            &invocation.source,
            &invocation.base_dir,
            invocation.sha256.as_deref(),
            ChannelProgress::as_dyn(progress.as_ref()),
        )
        .await
        .context(ResolveWasmSnafu)?;

    // 2. `run_plugin` preopens the `dirs` and reads the `files` itself — both
    //    anchored at `base_dir`, and both subject to the runtime's path-safety
    //    checks (no escaping or symlinked paths).
    let base_dir = Utf8PathBuf::from(invocation.base_dir.as_str());

    // 3. Run the plugin (blocking call — signal Tokio that this thread will block).
    let identity_principal = agent
        .get_principal()
        .map_err(|err| PluginError::GetIdentityPrincipal { err })?;

    let agent_clone = agent.clone();
    let dirs = invocation.dirs.clone();
    let files = invocation.files.clone();
    let cid = invocation.canister_id;
    let proxy = invocation.proxy;
    let environment_owned = invocation.environment.clone();
    let stdio_clone = stdio.clone();

    tokio::task::block_in_place(|| {
        run_plugin(
            wasm_path,
            base_dir,
            dirs,
            files,
            cid,
            agent_clone,
            proxy,
            identity_principal,
            environment_owned,
            stdio_clone,
        )
    })
    .context(RunSnafu)
}
