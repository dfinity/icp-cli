use std::collections::BTreeMap;

use camino::Utf8PathBuf;
use candid::Principal;
use ic_agent::Agent;
use icp_sync_plugin::{
    CallableCanisters, DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS, PLUGIN_COMPUTE_LIMIT_ENV,
    PluginInvocation, RunPluginError, run_plugin,
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

    #[snafu(display(
        "sync plugin lists canister '{name}' as callable, but no canister by that name \
         is known in environment '{environment}'"
    ))]
    UnknownCallableCanister { name: String, environment: String },
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

    // 3. Build the canister ID table exposed to the plugin, then resolve the
    //    step's `canisters` list against it.
    let canister_ids = exposed_canister_ids(params);
    let callable = resolve_callable(adapter, &canister_ids, environment)?;

    // 4. Run the plugin (blocking call — signal Tokio that this thread will block).
    let identity_principal = agent
        .get_principal()
        .map_err(|err| PluginError::GetIdentityPrincipal { err })?;

    let agent_clone = agent.clone();
    let environment_owned = environment.to_owned();
    let stdio_clone = stdio.clone();

    tokio::task::block_in_place(|| {
        run_plugin(PluginInvocation {
            wasm_path,
            base_dir,
            dirs,
            files,
            host_canister_id: params.cid,
            agent: agent_clone,
            proxy,
            identity_principal,
            environment: environment_owned,
            compute_limit_secs,
            canister_ids,
            callable,
            stdio: stdio_clone,
        })
    })
    .context(RunSnafu)
}

/// The canister ID table exposed to a sync plugin: every named canister in the
/// project, plus — for canisters in the same subproject as the one being synced
/// — a duplicate entry under the bare local name. A store key is
/// `<subproject>:<local>` for a canister in a subproject and a bare local name
/// for a canister defined directly in the app root (see the WIT
/// `canister-id-entry` docs), so the syncing canister's namespace is the prefix
/// of its own key.
///
/// A local name never contains a colon but a subproject directory may, so keys
/// split on their *last* colon. The bare-name aliases take precedence over an
/// app-root canister of the same local name: a plugin resolving a bare name is
/// naming what the syncing canister's own manifest calls it.
fn exposed_canister_ids(params: &Params) -> BTreeMap<String, Principal> {
    let syncing_namespace = params.name.rsplit_once(':').map(|(namespace, _)| namespace);

    let mut table = params.canister_ids.clone();
    for (key, id) in &params.canister_ids {
        if let Some((namespace, local)) = key.rsplit_once(':')
            && Some(namespace) == syncing_namespace
        {
            table.insert(local.to_owned(), *id);
        }
    }
    table
}

/// Resolve the step's `canisters` list into a [`CallableCanisters`] enforcement
/// set. Each listed name is looked up in `canister_ids`; a name that does not
/// resolve is a manifest error.
fn resolve_callable(
    adapter: &Adapter,
    canister_ids: &BTreeMap<String, Principal>,
    environment: &str,
) -> Result<CallableCanisters, PluginError> {
    let mut by_name = BTreeMap::new();
    for name in adapter.canisters.iter().flatten() {
        let principal = canister_ids
            .get(name)
            .copied()
            .context(UnknownCallableCanisterSnafu {
                name: name.clone(),
                environment: environment.to_owned(),
            })?;
        by_name.insert(name.clone(), principal);
    }
    Ok(CallableCanisters { by_name })
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

    use crate::manifest::adapter::prebuilt::{LocalSource, SourceField};

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[byte; 4])
    }

    fn params_named(name: &str, ids: &[(&str, Principal)]) -> Params {
        Params {
            path: "/work".into(),
            cid: principal(0),
            name: name.to_owned(),
            environment: "demo".to_owned(),
            network: "ic".to_owned(),
            canister_ids: ids.iter().map(|(n, p)| ((*n).to_owned(), *p)).collect(),
            proxy: None,
        }
    }

    fn adapter_with(canisters: Option<Vec<String>>) -> Adapter {
        Adapter {
            source: SourceField::Local(LocalSource {
                path: "plugin.wasm".into(),
            }),
            sha256: None,
            dirs: None,
            files: None,
            canisters,
        }
    }

    /// Canisters sharing the syncing canister's subproject are additionally
    /// exposed under their bare local name; canisters in other subprojects are
    /// not.
    #[test]
    fn exposed_ids_add_bare_names_for_same_subproject() {
        let backend = principal(1);
        let frontend = principal(2);
        let foreign = principal(3);
        let params = params_named(
            "services/open-accounts:backend",
            &[
                ("services/open-accounts:backend", backend),
                ("services/open-accounts:frontend", frontend),
                ("services/open-crm:backend", foreign),
            ],
        );

        let table = exposed_canister_ids(&params);

        // Same-subproject canisters gain a bare-local duplicate...
        assert_eq!(table.get("backend"), Some(&backend));
        assert_eq!(table.get("frontend"), Some(&frontend));
        // ...while the fully-qualified keys are still present for everyone.
        assert_eq!(
            table.get("services/open-accounts:frontend"),
            Some(&frontend)
        );
        assert_eq!(table.get("services/open-crm:backend"), Some(&foreign));
        // The other subproject's canister is not reachable by a bare name; the
        // bare "backend" belongs to the syncing canister's own subproject.
        assert_eq!(table.get("backend"), Some(&backend));
    }

    /// An app-root canister sharing a local name with a sibling of the syncing
    /// canister does not keep the bare name: the syncing subproject's own
    /// canister is what that name means to the plugin.
    #[test]
    fn exposed_ids_sibling_alias_overrides_the_app_root_name() {
        let root_backend = principal(1);
        let sibling_backend = principal(2);
        let params = params_named(
            "services/open-accounts:frontend",
            &[
                ("backend", root_backend),
                ("services/open-accounts:backend", sibling_backend),
                ("services/open-accounts:frontend", principal(3)),
            ],
        );

        let table = exposed_canister_ids(&params);

        assert_eq!(table.get("backend"), Some(&sibling_backend));
        // The app-root canister's only key was that bare name, so it drops out
        // of the table entirely rather than answering to a sibling's name.
        assert!(!table.values().any(|id| *id == root_backend));
    }

    /// A subproject directory may itself contain a colon, so keys are split on
    /// their last one — the same rule bundling uses.
    #[test]
    fn exposed_ids_split_subproject_prefix_at_the_last_colon() {
        let backend = principal(1);
        let frontend = principal(2);
        let params = params_named(
            "services/odd:name:backend",
            &[
                ("services/odd:name:backend", backend),
                ("services/odd:name:frontend", frontend),
            ],
        );

        let table = exposed_canister_ids(&params);

        assert_eq!(table.get("backend"), Some(&backend));
        assert_eq!(table.get("frontend"), Some(&frontend));
    }

    /// A single-project layout keys canisters by bare local name already, so no
    /// duplicates are added.
    #[test]
    fn exposed_ids_unchanged_without_a_subproject() {
        let backend = principal(1);
        let params = params_named("backend", &[("backend", backend)]);
        let table = exposed_canister_ids(&params);
        assert_eq!(table.len(), 1);
        assert_eq!(table.get("backend"), Some(&backend));
    }

    #[test]
    fn resolve_callable_resolves_names() {
        let dep = principal(1);
        let sibling = principal(2);
        let table = BTreeMap::from([
            ("backend".to_owned(), sibling),
            ("services/open-crm:backend".to_owned(), dep),
        ]);
        let adapter = adapter_with(Some(vec![
            "backend".to_owned(),
            "services/open-crm:backend".to_owned(),
        ]));

        let callable = resolve_callable(&adapter, &table, "demo").unwrap();

        assert_eq!(callable.by_name.get("backend"), Some(&sibling));
        assert_eq!(
            callable.by_name.get("services/open-crm:backend"),
            Some(&dep)
        );
    }

    #[test]
    fn resolve_callable_rejects_unknown_name() {
        let adapter = adapter_with(Some(vec!["nope".to_owned()]));
        let err = resolve_callable(&adapter, &BTreeMap::new(), "demo")
            .expect_err("an undeclared name must fail");
        assert!(matches!(err, PluginError::UnknownCallableCanister { .. }));
    }
}
