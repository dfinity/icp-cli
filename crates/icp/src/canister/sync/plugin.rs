use camino::Utf8PathBuf;
use candid::Principal;
use ic_agent::Agent;
use icp_sync_plugin::{
    DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS, PLUGIN_COMPUTE_LIMIT_ENV, RunPluginError, run_plugin,
};
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::{canister::wasm, manifest::adapter::plugin::Adapter, package::PackageCache};

use super::Params;

#[derive(Debug, Snafu)]
pub enum PluginError {
    #[snafu(transparent)]
    Wasm { source: wasm::WasmError },

    #[snafu(display("failed to get identity principal: {err}"))]
    GetIdentityPrincipal { err: String },

    #[snafu(display(
        "invalid {PLUGIN_COMPUTE_LIMIT_ENV} value '{value}': expected a positive integer number of seconds"
    ))]
    InvalidComputeLimit { value: String },

    #[snafu(display("failed to run plugin"))]
    Run { source: RunPluginError },
}

/// Resolve the plugin compute-time limit, honoring the
/// [`PLUGIN_COMPUTE_LIMIT_ENV`] override. Fails loudly on a malformed value so
/// a typo doesn't silently fall back to the default and leave the caller
/// wondering why their raised limit had no effect.
fn resolve_compute_limit_secs() -> Result<u64, PluginError> {
    match std::env::var(PLUGIN_COMPUTE_LIMIT_ENV) {
        Err(_) => Ok(DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS),
        Ok(value) => parse_compute_limit(&value),
    }
}

fn parse_compute_limit(value: &str) -> Result<u64, PluginError> {
    match value.trim().parse::<u64>() {
        Ok(secs) if secs >= 1 => Ok(secs),
        _ => InvalidComputeLimitSnafu {
            value: value.to_owned(),
        }
        .fail(),
    }
}

pub(super) async fn sync(
    adapter: &Adapter,
    params: &Params,
    agent: &Agent,
    environment: &str,
    proxy: Option<Principal>,
    stdio: Option<Sender<String>>,
    pkg_cache: &PackageCache,
) -> Result<Vec<String>, PluginError> {
    // 0. Resolve the compute-time limit up front so a malformed
    //    ICP_CLI_PLUGIN_COMPUTE_LIMIT_SECS fails fast — before downloading the
    //    wasm or touching the network — rather than after doing that work.
    let compute_limit_secs = resolve_compute_limit_secs()?;

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

    // 2. Collect inputs as manifest strings. `run_plugin` preopens the `dirs`
    //    and reads the `files` itself — both anchored at `base_dir`, and both
    //    subject to the runtime's path-safety checks (no escaping or symlinked
    //    paths).
    let base_dir = Utf8PathBuf::from(params.path.as_str());
    let dirs: Vec<String> = adapter.dirs.clone().unwrap_or_default();
    let files: Vec<String> = adapter.files.clone().unwrap_or_default();

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
            compute_limit_secs,
            stdio_clone,
        )
    })
    .context(RunSnafu)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_compute_limit_accepts_positive_integers() {
        assert_eq!(parse_compute_limit("300").unwrap(), 300);
        // Surrounding whitespace is tolerated.
        assert_eq!(parse_compute_limit("  42 ").unwrap(), 42);
    }

    #[test]
    fn parse_compute_limit_rejects_invalid_values() {
        for bad in ["0", "abc", "30O", "-5", "1.5", ""] {
            let err =
                parse_compute_limit(bad).expect_err(&format!("expected '{bad}' to be rejected"));
            assert!(
                matches!(err, PluginError::InvalidComputeLimit { .. }),
                "unexpected error for '{bad}': {err}"
            );
        }
    }
}
