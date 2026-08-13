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
//! [`crate::project::verify_sandbox`] before they reach an executor; [`NoScripts`]
//! is the ready-made [`ScriptRunner`] for a host that has no subprocesses.

use std::collections::BTreeMap;

use async_trait::async_trait;
use candid::Principal;
use snafu::Snafu;

use crate::manifest::adapter::{plugin, prebuilt::SourceField, script};
use crate::prelude::*;

/// Resolved context for executing one canister's sync steps.
#[derive(Clone, Debug)]
pub struct SyncStepContext {
    /// Directory the canister was declared in (base for relative plugin paths).
    pub canister_path: PathBuf,
    /// The canister being synced.
    pub canister_id: Principal,
    /// Name of the environment being synced (e.g. "local", "production").
    pub environment: String,
    /// Name of the network (e.g. "local", "ic").
    pub network: String,
    /// IDs of all named canisters in the project for this environment.
    pub canister_ids: BTreeMap<String, Principal>,
    /// Proxy canister to route calls through, if `--proxy` was passed.
    pub proxy: Option<Principal>,
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
    pub dirs: Vec<String>,
    /// Files the host reads and passes inline to the plugin.
    pub files: Vec<String>,
    /// The canister the plugin may call.
    pub canister_id: Principal,
    /// Environment name exposed to the plugin via its `SyncExecInput`.
    pub environment: String,
    /// Proxy canister to route the plugin's canister calls through, if any.
    pub proxy: Option<Principal>,
}

impl PluginInvocation {
    /// Resolve a plugin step's adapter against the sync context.
    pub fn new(adapter: &plugin::Adapter, ctx: &SyncStepContext) -> Self {
        Self {
            source: adapter.source.clone(),
            sha256: adapter.sha256.clone(),
            base_dir: ctx.canister_path.clone(),
            dirs: adapter.dirs.clone().unwrap_or_default(),
            files: adapter.files.clone().unwrap_or_default(),
            canister_id: ctx.canister_id,
            environment: ctx.environment.clone(),
            proxy: ctx.proxy,
        }
    }
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

/// A [`ScriptRunner`] that always fails, for environments that don't support
/// subprocess script sync steps.
pub struct NoScripts;

#[derive(Debug, Snafu)]
#[snafu(display("script sync steps are not supported in this environment"))]
pub struct NoScriptsError;

#[cfg_attr(target_family = "wasm", async_trait(?Send))]
#[cfg_attr(not(target_family = "wasm"), async_trait)]
impl ScriptRunner for NoScripts {
    async fn run_script(
        &self,
        _invocation: ScriptInvocation,
        _progress: Option<&dyn StepProgress>,
    ) -> Result<Vec<String>, ScriptRunError> {
        Err(ScriptRunError {
            source: Box::new(NoScriptsError),
        })
    }
}
