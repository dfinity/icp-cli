//! Injected sync-step execution.
//!
//! A canister's sync steps run either a WASI plugin (wasmtime) or a subprocess
//! script — neither can run inside a canister — so their execution is provided
//! by the host through [`PluginExecutor`] and [`ScriptRunner`]. This crate keeps
//! *all* of the derivation, though: it dispatches on the step kind, resolves the
//! plugin inputs, and assembles the `ICP_CLI_*` system environment variables
//! scripts run with. The host implementations only perform the irreducible host
//! action — fetch-and-run-the-wasm, or spawn-the-subprocess — against a
//! fully-resolved [`PluginInvocation`] / [`ScriptInvocation`].
//!
//! The two executors are separate traits because an environment can support one
//! without the other. Script steps are host-only, and are rejected by
//! [`crate::project::verify_sandbox`] before they reach an executor.

use std::collections::BTreeMap;

use async_trait::async_trait;
use candid::Principal;
use snafu::prelude::*;

use crate::manifest::adapter::{
    plugin::{self, NamedPaths},
    prebuilt::SourceField,
    script,
};
use crate::prelude::*;

/// Resolved context for executing one canister's sync steps.
#[derive(Clone, Debug)]
pub struct SyncStepContext {
    /// Directory the canister was declared in (base for relative plugin paths).
    pub canister_path: PathBuf,
    /// The canister being synced.
    pub canister_id: Principal,
    /// Store key of the canister being synced (e.g. `backend`, or
    /// `services/open-crm:backend` for a canister in a subproject) — the `name`
    /// of its [`Canister`](crate::Canister). Its namespace prefix
    /// identifies which other canisters are in the same subproject.
    pub canister_name: String,
    /// Name of the environment being synced (e.g. "local", "production").
    pub environment: String,
    /// Name of the network (e.g. "local", "ic").
    pub network: String,
    /// IDs of all named canisters in the project for this environment.
    pub canister_ids: BTreeMap<String, Principal>,
    /// Proxy canister to route calls through, if `--proxy` was passed.
    pub proxy: Option<Principal>,
}

/// A manifest-declared path, tagged with the map key it was declared under.
/// A plain-list entry carries no key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyedPath {
    /// The `dirs:`/`files:` map key this path sits under, or `None` for a
    /// plain-list entry. Non-unique: the paths of a key that maps to a list all
    /// share it.
    pub key: Option<String>,
    /// The path itself, relative to the canister directory.
    pub path: String,
}

/// Convert a manifest [`NamedPaths`] (or its absence) into the key-tagged path
/// list the executor receives. A missing setting yields an empty list.
fn keyed_paths(paths: Option<&NamedPaths>) -> Vec<KeyedPath> {
    paths
        .into_iter()
        .flat_map(NamedPaths::entries)
        .map(|entry| KeyedPath {
            key: entry.key.map(str::to_string),
            path: entry.path.to_string(),
        })
        .collect()
}

/// A fully-resolved WASI-plugin sync step. Everything the host needs to fetch
/// and run the plugin has been computed by this crate; the host supplies only
/// the wasm source resolution (through its
/// [`RemoteResourceResolve`](crate::canister::recipe::RemoteResourceResolve))
/// and the wasmtime runtime, plus its own identity/agent state.
#[derive(Clone, Debug)]
pub struct PluginInvocation {
    /// Where the plugin wasm comes from (local path or remote URL).
    pub source: SourceField,
    /// Optional sha256 the host verifies the wasm against (required for remote).
    pub sha256: Option<String>,
    /// Canister directory; base for the relative `dirs`/`files` and the source.
    pub base_dir: PathBuf,
    /// Directories preopened read-only into the WASI sandbox.
    pub dirs: Vec<KeyedPath>,
    /// Files the host reads and passes inline to the plugin.
    pub files: Vec<KeyedPath>,
    /// Key-value fields passed inline to the plugin.
    pub fields: BTreeMap<String, String>,
    /// The canister being synced, which the plugin may always call.
    pub canister_id: Principal,
    /// Environment name exposed to the plugin via its `SyncExecInput`.
    pub environment: String,
    /// The canister ID table exposed to the plugin: every named canister in
    /// the project, plus a bare-local-name alias for each canister in the same
    /// subproject as the one being synced.
    pub canister_ids: BTreeMap<String, Principal>,
    /// The canisters the step's `canisters:` list named, resolved to ids. These
    /// are callable in addition to [`canister_id`](Self::canister_id).
    pub callable: BTreeMap<String, Principal>,
    /// Proxy canister to route the plugin's canister calls through, if any.
    pub proxy: Option<Principal>,
}

/// A plugin step named a canister in its `canisters:` list that the environment
/// does not have.
#[derive(Debug, Snafu)]
#[snafu(display(
    "sync plugin lists canister '{name}' as callable, but no canister by that name \
     is known in environment '{environment}'"
))]
pub struct UnknownCallableCanisterError {
    name: String,
    environment: String,
}

impl PluginInvocation {
    /// Resolve a plugin step's adapter against the sync context. Fails if the
    /// step declares a callable canister the environment does not have.
    pub fn new(
        adapter: &plugin::Adapter,
        ctx: &SyncStepContext,
    ) -> Result<Self, UnknownCallableCanisterError> {
        let canister_ids = exposed_canister_ids(ctx);
        let callable = resolve_callable(adapter, &canister_ids, &ctx.environment)?;
        Ok(Self {
            source: adapter.source.clone(),
            sha256: adapter.sha256.clone(),
            base_dir: ctx.canister_path.clone(),
            dirs: keyed_paths(adapter.dirs.as_ref()),
            files: keyed_paths(adapter.files.as_ref()),
            fields: adapter.fields.clone().unwrap_or_default(),
            canister_id: ctx.canister_id,
            environment: ctx.environment.clone(),
            canister_ids,
            callable,
            proxy: ctx.proxy,
        })
    }
}

/// The canister ID table exposed to a sync plugin: every named canister in the
/// project, plus — for canisters in the same subproject as the one being synced
/// — a duplicate entry under the bare local name. A store key is
/// `<subproject>:<local>` for a canister in a subproject and a bare local name
/// for a canister defined directly in the app root, so the syncing canister's
/// namespace is the prefix of its own key.
///
/// A local name never contains a colon but a subproject directory may, so keys
/// split on their *last* colon. The bare-name aliases take precedence over an
/// app-root canister of the same local name: a plugin resolving a bare name is
/// naming what the syncing canister's own manifest calls it.
fn exposed_canister_ids(ctx: &SyncStepContext) -> BTreeMap<String, Principal> {
    let syncing_namespace = ctx
        .canister_name
        .rsplit_once(':')
        .map(|(namespace, _)| namespace);

    let mut table = ctx.canister_ids.clone();
    for (key, id) in &ctx.canister_ids {
        if let Some((namespace, local)) = key.rsplit_once(':')
            && Some(namespace) == syncing_namespace
        {
            table.insert(local.to_owned(), *id);
        }
    }
    table
}

/// Resolve the step's `canisters:` list against `canister_ids`. A name that does
/// not resolve is a manifest error.
fn resolve_callable(
    adapter: &plugin::Adapter,
    canister_ids: &BTreeMap<String, Principal>,
    environment: &str,
) -> Result<BTreeMap<String, Principal>, UnknownCallableCanisterError> {
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
    Ok(by_name)
}

/// A fully-resolved script sync step. This crate has already assembled the
/// working directory and the complete environment the subprocess runs with
/// (see [`system_env_vars`]); the host only spawns the command(s).
#[derive(Clone, Debug)]
pub struct ScriptInvocation {
    /// Shell command(s) to run in order.
    pub commands: Vec<String>,
    /// Working directory (the canister directory).
    pub cwd: PathBuf,
    /// Environment variables the subprocess inherits, in insertion order.
    pub env: Vec<(String, String)>,
}

impl ScriptInvocation {
    /// Resolve a script step's adapter against the sync context, assembling the
    /// `ICP_CLI_*` system environment variables the command runs with.
    pub fn new(adapter: &script::Adapter, ctx: &SyncStepContext) -> Self {
        Self {
            commands: adapter.command.as_vec(),
            cwd: ctx.canister_path.clone(),
            env: system_env_vars(ctx),
        }
    }
}

/// The `ICP_CLI_*` system environment variables every script sync step runs
/// with: the environment and network names, the target canister id, and one
/// `ICP_CLI_CID_<NAME>` per known canister in the environment (name uppercased,
/// non-alphanumerics replaced with `_`).
pub fn system_env_vars(ctx: &SyncStepContext) -> Vec<(String, String)> {
    let mut envs = vec![
        ("ICP_CLI_ENVIRONMENT".to_owned(), ctx.environment.clone()),
        ("ICP_CLI_NETWORK".to_owned(), ctx.network.clone()),
        ("ICP_CLI_CID".to_owned(), ctx.canister_id.to_text()),
    ];
    for (name, id) in &ctx.canister_ids {
        let key = format!(
            "ICP_CLI_CID_{}",
            name.to_uppercase()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>()
        );
        envs.push((key, id.to_text()));
    }
    envs
}

/// A sink for streamed sync-step output lines (a presentation concern the host
/// implements, e.g. over a progress bar).
pub trait StepProgress: Send + Sync {
    fn line(&self, line: String);
}

/// A plugin step failed. The concrete cause (a host wasm/runtime error) is boxed
/// because this crate does not depend on the executor's implementation; callers
/// can still walk `source()`.
#[derive(Debug, Snafu)]
#[snafu(display("plugin sync step failed"))]
pub struct PluginExecutorError {
    pub source: Box<dyn std::error::Error + Send + Sync + 'static>,
}

/// Host execution of WASI-plugin sync steps.
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
pub trait PluginExecutor: Send + Sync {
    /// Fetch and run a WASI plugin against a canister, returning any stderr
    /// lines the plugin emitted that should be retained past the streamed view.
    async fn run_plugin(
        &self,
        invocation: PluginInvocation,
        progress: Option<&dyn StepProgress>,
    ) -> Result<Vec<String>, PluginExecutorError>;
}

/// A script step failed. Boxed for the same reason as [`PluginExecutorError`].
#[derive(Debug, Snafu)]
#[snafu(display("script sync step failed"))]
pub struct ScriptRunError {
    pub source: Box<dyn std::error::Error + Send + Sync + 'static>,
}

/// Host execution of subprocess script sync steps.
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
pub trait ScriptRunner: Send + Sync {
    /// Run a resolved script step, returning any stderr lines to retain past the
    /// streamed view.
    async fn run_script(
        &self,
        invocation: ScriptInvocation,
        progress: Option<&dyn StepProgress>,
    ) -> Result<Vec<String>, ScriptRunError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::adapter::prebuilt::{LocalSource, SourceField};

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[byte; 4])
    }

    fn ctx_named(name: &str, ids: &[(&str, Principal)]) -> SyncStepContext {
        SyncStepContext {
            canister_path: "/work".into(),
            canister_id: principal(0),
            canister_name: name.to_owned(),
            environment: "demo".to_owned(),
            network: "ic".to_owned(),
            canister_ids: ids.iter().map(|(n, p)| ((*n).to_owned(), *p)).collect(),
            proxy: None,
        }
    }

    fn adapter_with(canisters: Option<Vec<String>>) -> plugin::Adapter {
        plugin::Adapter {
            source: SourceField::Local(LocalSource {
                path: "plugin.wasm".into(),
            }),
            sha256: None,
            dirs: None,
            files: None,
            fields: None,
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
        let ctx = ctx_named(
            "services/open-accounts:backend",
            &[
                ("services/open-accounts:backend", backend),
                ("services/open-accounts:frontend", frontend),
                ("services/open-crm:backend", foreign),
            ],
        );

        let table = exposed_canister_ids(&ctx);

        // Same-subproject canisters gain a bare-local duplicate...
        assert_eq!(table.get("backend"), Some(&backend));
        assert_eq!(table.get("frontend"), Some(&frontend));
        // ...while the fully-qualified keys are still present for everyone.
        assert_eq!(
            table.get("services/open-accounts:frontend"),
            Some(&frontend)
        );
        assert_eq!(table.get("services/open-crm:backend"), Some(&foreign));
    }

    /// An app-root canister sharing a local name with a sibling of the syncing
    /// canister does not keep the bare name: the syncing subproject's own
    /// canister is what that name means to the plugin.
    #[test]
    fn exposed_ids_sibling_alias_overrides_the_app_root_name() {
        let root_backend = principal(1);
        let sibling_backend = principal(2);
        let ctx = ctx_named(
            "services/open-accounts:frontend",
            &[
                ("backend", root_backend),
                ("services/open-accounts:backend", sibling_backend),
                ("services/open-accounts:frontend", principal(3)),
            ],
        );

        let table = exposed_canister_ids(&ctx);

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
        let ctx = ctx_named(
            "services/odd:name:backend",
            &[
                ("services/odd:name:backend", backend),
                ("services/odd:name:frontend", frontend),
            ],
        );

        let table = exposed_canister_ids(&ctx);

        assert_eq!(table.get("backend"), Some(&backend));
        assert_eq!(table.get("frontend"), Some(&frontend));
    }

    /// A single-project layout keys canisters by bare local name already, so no
    /// duplicates are added.
    #[test]
    fn exposed_ids_unchanged_without_a_subproject() {
        let backend = principal(1);
        let ctx = ctx_named("backend", &[("backend", backend)]);
        let table = exposed_canister_ids(&ctx);
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

        assert_eq!(callable.get("backend"), Some(&sibling));
        assert_eq!(callable.get("services/open-crm:backend"), Some(&dep));
    }

    #[test]
    fn resolve_callable_rejects_unknown_name() {
        let adapter = adapter_with(Some(vec!["nope".to_owned()]));
        resolve_callable(&adapter, &BTreeMap::new(), "demo")
            .expect_err("an undeclared name must fail");
    }

    /// A script step resolves to the manifest's commands, the canister directory
    /// as cwd, and the `ICP_CLI_*` environment assembled from the context.
    #[test]
    fn script_invocation_resolves_commands_cwd_and_env() {
        use crate::manifest::adapter::script::{Adapter, CommandField};

        let cid = principal(7);
        let frontend = principal(8);
        let ctx = SyncStepContext {
            canister_path: "/work/backend".into(),
            canister_id: cid,
            canister_name: "backend".to_owned(),
            environment: "production".to_owned(),
            network: "ic".to_owned(),
            canister_ids: BTreeMap::from([("my-frontend".to_owned(), frontend)]),
            proxy: None,
        };
        let adapter = Adapter {
            command: CommandField::Command("./deploy.sh".to_owned()),
        };

        let invocation = ScriptInvocation::new(&adapter, &ctx);

        assert_eq!(invocation.commands, vec!["./deploy.sh"]);
        assert_eq!(invocation.cwd, PathBuf::from("/work/backend"));
        assert_eq!(
            invocation.env,
            vec![
                ("ICP_CLI_ENVIRONMENT".to_owned(), "production".to_owned()),
                ("ICP_CLI_NETWORK".to_owned(), "ic".to_owned()),
                ("ICP_CLI_CID".to_owned(), cid.to_text()),
                ("ICP_CLI_CID_MY_FRONTEND".to_owned(), frontend.to_text()),
            ]
        );
    }
}
