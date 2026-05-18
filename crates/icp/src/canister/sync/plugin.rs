use camino::Utf8PathBuf;
use candid::Principal;
use ic_agent::Agent;
use icp_sync_plugin::{RunPluginError, run_plugin};
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::{
    canister::wasm, fs::read_to_string, manifest::adapter::plugin::Adapter, package::PackageCache,
};

use super::Params;

#[derive(Debug, Snafu)]
pub enum PluginError {
    #[snafu(display(
        "plugin file path '{name}' is not a safe relative path (no absolute paths or '..' allowed)"
    ))]
    UnsafeFilePath { name: String },

    #[snafu(display("failed to read plugin input file at '{path}'"))]
    ReadFile {
        source: crate::fs::IoError,
        path: Utf8PathBuf,
    },

    #[snafu(transparent)]
    Wasm { source: wasm::WasmError },

    #[snafu(display("failed to get identity principal: {err}"))]
    GetIdentityPrincipal { err: String },

    #[snafu(display("failed to run plugin"))]
    Run { source: RunPluginError },
}

pub(super) async fn sync(
    adapter: &Adapter,
    params: &Params,
    agent: &Agent,
    environment: &str,
    proxy: Option<Principal>,
    stdio: Option<Sender<String>>,
    pkg_cache: &PackageCache,
) -> Result<(), PluginError> {
    // 1. Determine the on-disk path for the wasm. run_plugin needs a path, not raw bytes.
    //    - Local: sha256 is verified if present, then the original path is returned.
    //    - Remote: downloaded to cache (sha256 required, enforced at parse time) and the
    //      stable cache path is returned — no temp file needed.
    let wasm_path = wasm::resolve(
        &adapter.source,
        &params.path,
        adapter.sha256.as_deref(),
        stdio.as_ref(),
        pkg_cache,
    )
    .await?;

    // 2. Collect inputs: `dirs` stays as manifest strings (runtime preopens them),
    //    `files` are read on the host and passed inline.
    let base_dir = Utf8PathBuf::from(params.path.as_str());
    let dirs: Vec<String> = adapter.dirs.clone().unwrap_or_default();

    let mut files: Vec<(String, String)> = Vec::new();
    for name in adapter.files.as_deref().unwrap_or(&[]) {
        let p = Utf8PathBuf::from(name);
        ensure!(
            !p.is_absolute()
                && !p
                    .components()
                    .any(|c| c == camino::Utf8Component::ParentDir),
            UnsafeFilePathSnafu { name }
        );
        let abs = params.path.join(name);
        let content = read_to_string(abs.as_ref()).context(ReadFileSnafu { path: abs })?;
        files.push((name.clone(), content));
    }

    // 3. Run the plugin (blocking call — signal Tokio that this thread will block).
    let identity_principal = agent
        .get_principal()
        .map_err(|err| PluginError::GetIdentityPrincipal { err })?;

    let agent_clone = agent.clone();
    let environment_owned = environment.to_owned();
    let stdio_clone = stdio.clone();

    tokio::task::block_in_place(|| {
        run_plugin(
            wasm_path,
            base_dir,
            dirs,
            files,
            params.cid,
            agent_clone,
            proxy,
            identity_principal,
            environment_owned,
            stdio_clone,
        )
    })
    .context(RunSnafu)
}
