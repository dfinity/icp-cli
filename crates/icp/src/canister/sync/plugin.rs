use camino::Utf8PathBuf;
use candid::Principal;
use ic_agent::Agent;
use icp_sync_plugin::{RunPluginError, run_plugin};
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::{
    canister::wasm,
    fs::{read_to_string, write},
    manifest::adapter::{plugin::Adapter, prebuilt::SourceField},
    package::PackageCache,
};

use super::Params;

#[derive(Debug, Snafu)]
pub enum PluginError {
    #[snafu(display("failed to read plugin input file at '{path}'"))]
    ReadFile {
        source: crate::fs::IoError,
        path: Utf8PathBuf,
    },

    #[snafu(display("failed to write downloaded plugin wasm to temp file"))]
    WriteTempWasm { source: crate::fs::IoError },

    #[snafu(transparent)]
    Wasm { source: wasm::WasmError },

    #[snafu(display("failed to get identity principal: {err}"))]
    GetIdentityPrincipal { err: String },

    #[snafu(display("failed to acquire lock on package cache"))]
    LockCache { source: crate::fs::lock::LockError },

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
    //    - Local: use the manifest path directly.
    //    - Remote + sha256 known: resolve via cache (download once, reuse thereafter);
    //      the stable cache path avoids a temp file.
    //    - Remote + no sha256: download and write to a temp file (cleaned up after).
    let (wasm_path, is_temp) = match &adapter.source {
        SourceField::Local(s) => (params.path.join(&s.path), false),
        SourceField::Remote(_) => match &adapter.sha256 {
            Some(sha) => {
                wasm::resolve(
                    &adapter.source,
                    &params.path,
                    Some(sha),
                    stdio.as_ref(),
                    pkg_cache,
                )
                .await?;
                let path = wasm::cached_path(pkg_cache, sha)
                    .await
                    .context(LockCacheSnafu)?;
                (path, false)
            }
            None => {
                let wasm_bytes = wasm::resolve(
                    &adapter.source,
                    &params.path,
                    None,
                    stdio.as_ref(),
                    pkg_cache,
                )
                .await?;
                let tmp = params.path.join(format!(
                    ".icp-plugin-{}.wasm",
                    hex::encode(&wasm_bytes[..std::cmp::min(8, wasm_bytes.len())])
                ));
                write(tmp.as_ref(), &wasm_bytes).context(WriteTempWasmSnafu)?;
                (tmp, true)
            }
        },
    };

    // 2. Collect inputs: `dirs` stays as manifest strings (runtime preopens them),
    //    `files` are read on the host and passed inline.
    let base_dir = Utf8PathBuf::from(params.path.as_str());
    let dirs: Vec<String> = adapter.dirs.clone().unwrap_or_default();

    let mut files: Vec<(String, String)> = Vec::new();
    for name in adapter.files.as_deref().unwrap_or(&[]) {
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
            wasm_path.clone(),
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
    .context(RunSnafu)?;

    // Clean up temp file if we downloaded from a remote URL with no sha256.
    if is_temp {
        let _ = std::fs::remove_file(wasm_path.as_std_path());
    }

    Ok(())
}
