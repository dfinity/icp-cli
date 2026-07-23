//! Sync-step support: the resolved step context, the `ICP_CLI_*` script
//! environment, and the host seams the step loop still needs.
//!
//! Plugin steps run inside the sandboxed wasmtime engine, which [`run_sync_steps`]
//! drives directly. Two things it can't do itself stay behind host seams:
//! subprocess script steps (which may be disallowed in a sandboxed environment)
//! go through [`ScriptRunner`], and step framing / output streaming goes through
//! [`StepProgress`].
//!
//! [`run_sync_steps`]: crate::deploy::run_sync_steps

use std::collections::BTreeMap;

use async_trait::async_trait;
use candid::Principal;
use snafu::Snafu;
use tokio::sync::mpsc::Sender;

use crate::manifest::adapter::script;
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

/// A fully-resolved script sync step: the command(s), the working directory, and
/// the complete environment the subprocess runs with (see [`system_env_vars`]).
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

/// Per-step framing and output streaming, implemented by the host over its
/// progress display. Each step is bracketed by `begin_step`/`end_step`; the
/// `Sender` returned by `begin_step` receives the step's streamed output lines.
#[async_trait]
pub trait StepProgress: Send {
    /// Start a step with the given header, returning a sink for its output lines
    /// (or `None` to discard output).
    fn begin_step(&mut self, header: String) -> Option<Sender<String>>;

    /// Finish the current step.
    async fn end_step(&mut self);
}

#[derive(Debug, Snafu)]
#[snafu(display("script sync step failed"))]
pub struct ScriptRunError {
    pub source: Box<dyn std::error::Error + Send + Sync + 'static>,
}

/// Host execution of subprocess script sync steps. Kept behind a trait because a
/// sandboxed environment may forbid spawning processes; the wasmtime plugin
/// engine, by contrast, is driven directly by [`run_sync_steps`].
///
/// [`run_sync_steps`]: crate::deploy::run_sync_steps
#[async_trait]
pub trait ScriptRunner: Sync + Send {
    /// Run a resolved script step, streaming output to `stdio`, and return any
    /// stderr lines to retain past the streamed view.
    async fn run_script(
        &self,
        invocation: ScriptInvocation,
        stdio: Option<Sender<String>>,
    ) -> Result<Vec<String>, ScriptRunError>;
}

/// A [`ScriptRunner`] that always fails, for environments that don't support
/// subprocess script sync steps.
pub struct NoScripts;

#[derive(Debug, Snafu)]
#[snafu(display("script sync steps are not supported in this environment"))]
pub struct NoScriptsError;

#[async_trait]
impl ScriptRunner for NoScripts {
    async fn run_script(
        &self,
        _invocation: ScriptInvocation,
        _stdio: Option<Sender<String>>,
    ) -> Result<Vec<String>, ScriptRunError> {
        Err(ScriptRunError {
            source: Box::new(NoScriptsError),
        })
    }
}
