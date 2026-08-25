use camino::Utf8PathBuf;
use ic_agent::Agent;
use icp_deploy_canister::canister::recipe::{RemoteResourceResolve, ResolveError};
use icp_deploy_canister::sync_exec;
use icp_events::StepReporter;
use icp_sync_plugin::{
    CallableCanisters, DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS, KeyedPath, PLUGIN_COMPUTE_LIMIT_ENV,
    PluginInvocation, RunPluginError, run_plugin,
};
use snafu::prelude::*;

use crate::canister::ReporterProgress;

#[derive(Debug, Snafu)]
pub enum PluginError {
    #[snafu(display("failed to resolve plugin wasm"))]
    ResolveWasm { source: ResolveError },

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
        Ok(value) => parse_compute_limit(&value),
        // Only a genuinely unset variable selects the default. A variable that
        // is present but not valid UTF-8 is a malformed value, not "unset", so
        // it must be rejected to honor the fail-loudly contract.
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS),
        Err(std::env::VarError::NotUnicode(raw)) => InvalidComputeLimitSnafu {
            value: raw.to_string_lossy().into_owned(),
        }
        .fail(),
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

/// Restate the library's key-tagged paths as the runtime's. The two types are
/// identical by construction and separate only because the runtime crate cannot
/// be depended on from `icp-deploy-canister`.
fn keyed_paths(paths: &[sync_exec::KeyedPath]) -> Vec<KeyedPath> {
    paths
        .iter()
        .map(|entry| KeyedPath {
            key: entry.key.clone(),
            path: entry.path.clone(),
        })
        .collect()
}

/// Fetch and run a WASI plugin against a canister for a fully-resolved
/// [`sync_exec::PluginInvocation`]. Dispatch and input derivation — the
/// key-tagged paths, the fields, the exposed canister-id table and the resolved
/// `canisters:` list — happen in `icp-deploy-canister`; this only performs the
/// host-only wasm resolution and wasmtime execution.
pub(super) async fn run(
    invocation: &sync_exec::PluginInvocation,
    agent: &Agent,
    reporter: &StepReporter,
    resolver: &dyn RemoteResourceResolve,
) -> Result<Vec<String>, PluginError> {
    // 0. Resolve the compute-time limit up front so a malformed
    //    ICP_CLI_PLUGIN_COMPUTE_LIMIT_SECS fails fast — before downloading the
    //    wasm or touching the network — rather than after doing that work.
    let compute_limit_secs = resolve_compute_limit_secs()?;

    // 1. Determine the on-disk path for the wasm. run_plugin needs a path, not raw bytes.
    //    - Local: sha256 is verified if present, then the original path is returned.
    //    - Remote: downloaded to cache (sha256 required, enforced at parse time) and the
    //      stable cache path is returned — no temp file needed.
    let wasm_path = resolver
        .resolve_wasm(
            &invocation.source,
            &invocation.base_dir,
            invocation.sha256.as_deref(),
            Some(&ReporterProgress(reporter)),
        )
        .await
        .context(ResolveWasmSnafu)?;

    // 2. `run_plugin` preopens the `dirs` and reads the `files` itself — both
    //    anchored at `base_dir`, confined to `project_dir`, and subject to the
    //    runtime's path-safety checks (no escaping or symlinked paths).
    let base_dir = Utf8PathBuf::from(invocation.base_dir.as_str());
    let project_dir = Utf8PathBuf::from(invocation.project_dir.as_str());

    // 3. Run the plugin (blocking call — signal Tokio that this thread will block).
    let identity_principal = agent
        .get_principal()
        .map_err(|err| PluginError::GetIdentityPrincipal { err })?;

    let runtime_invocation = PluginInvocation {
        wasm_path,
        base_dir,
        project_dir,
        dirs: keyed_paths(&invocation.dirs),
        files: keyed_paths(&invocation.files),
        fields: invocation.fields.clone(),
        host_canister_id: invocation.canister_id,
        agent: agent.clone(),
        proxy: invocation.proxy,
        identity_principal,
        environment: invocation.environment.clone(),
        compute_limit_secs,
        canister_ids: invocation.canister_ids.clone(),
        callable: CallableCanisters {
            by_name: invocation.callable.clone(),
        },
        reporter: reporter.clone(),
    };

    tokio::task::block_in_place(|| run_plugin(runtime_invocation)).context(RunSnafu)
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
