//! Script sync steps, split into resolution and execution.
//!
//! [`ScriptInvocation::new`] resolves a manifest script step against the sync
//! [`Params`] — the command list, the working directory and the `ICP_CLI_*`
//! environment — without running anything. [`ScriptRunner`] then executes a
//! resolved invocation.
//!
//! Execution sits behind a trait because spawning a subprocess is the one part of
//! sync that cannot be done everywhere: plugin steps run inside the wasmtime WASI
//! sandbox, but a script step needs a shell. Keeping the split here means the
//! resolution half is portable and unit-testable, and an environment without
//! subprocesses can substitute a runner that refuses instead of losing the whole
//! sync path.

use async_trait::async_trait;
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::manifest::adapter::script::Adapter;
use crate::prelude::*;

use super::Params;

use super::super::script::execute_commands;

/// A fully-resolved script sync step: the command(s), the working directory, and
/// the environment variables to set for them (see [`system_env_vars`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptInvocation {
    /// Shell command(s) to run in order.
    pub commands: Vec<String>,
    /// Working directory (the canister directory).
    pub cwd: PathBuf,
    /// Environment variables a runner **adds to** its execution environment, in
    /// insertion order — not the complete environment. [`HostScripts`] overlays
    /// them onto the inherited process environment, so the script still sees
    /// ambient variables such as `PATH` and `HOME`; entries here win on a name
    /// collision. A runner is not expected to clear what it inherits.
    pub env: Vec<(String, String)>,
}

impl ScriptInvocation {
    /// Resolve a script step's adapter against the sync context, assembling the
    /// `ICP_CLI_*` system environment variables the command runs with.
    pub fn new(adapter: &Adapter, params: &Params) -> Self {
        Self {
            commands: adapter.command.as_vec(),
            cwd: params.path.clone(),
            env: system_env_vars(params),
        }
    }
}

/// The `ICP_CLI_*` system environment variables every script sync step runs
/// with: the environment and network names, the target canister id, and one
/// `ICP_CLI_CID_<NAME>` per known canister in the environment (name uppercased,
/// non-alphanumerics replaced with `_`).
pub fn system_env_vars(params: &Params) -> Vec<(String, String)> {
    let mut envs = vec![
        ("ICP_CLI_ENVIRONMENT".to_owned(), params.environment.clone()),
        ("ICP_CLI_NETWORK".to_owned(), params.network.clone()),
        ("ICP_CLI_CID".to_owned(), params.cid.to_text()),
    ];
    for (name, id) in &params.canister_ids {
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

#[derive(Debug, Snafu)]
#[snafu(display("script sync step failed"))]
pub struct ScriptRunError {
    /// Boxed because the error depends on how the runner executes: the host
    /// runner fails with a [`ScriptError`](super::super::script::ScriptError), a
    /// runner that refuses scripts fails with something else entirely.
    pub source: Box<dyn std::error::Error + Send + Sync + 'static>,
}

/// Executes resolved script sync steps.
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

/// The [`ScriptRunner`] that spawns each command as a host subprocess.
pub struct HostScripts;

#[async_trait]
impl ScriptRunner for HostScripts {
    async fn run_script(
        &self,
        invocation: ScriptInvocation,
        stdio: Option<Sender<String>>,
    ) -> Result<Vec<String>, ScriptRunError> {
        let env_refs: Vec<(&str, &str)> = invocation
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        execute_commands(&invocation.commands, &invocation.cwd, &env_refs, stdio)
            .await
            .map_err(|source| ScriptRunError {
                source: Box::new(source),
            })?;
        // Persistent stderr is a sync-plugin feature only; script steps don't
        // currently retain any output past the rolling step view.
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tokio::sync::Mutex;

    use candid::Principal;

    use super::*;
    use crate::manifest::adapter::script::CommandField;

    /// Serializes the tests here that mutate the process environment, since
    /// cargo runs tests in parallel threads. Async-aware because the variable
    /// has to stay set across the subprocess `await` that reads it.
    static ENV_MUTEX: Mutex<()> = Mutex::const_new(());

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[byte; 4])
    }

    fn params(canister_ids: &[(&str, Principal)]) -> Params {
        Params {
            path: "/work/backend".into(),
            cid: principal(1),
            name: "backend".to_owned(),
            environment: "production".to_owned(),
            network: "ic".to_owned(),
            canister_ids: canister_ids
                .iter()
                .map(|(n, p)| ((*n).to_owned(), *p))
                .collect::<BTreeMap<_, _>>(),
            proxy: None,
        }
    }

    /// The environment, network and target canister id are always present.
    #[test]
    fn base_env_vars_are_always_set() {
        let p = params(&[]);
        assert_eq!(
            system_env_vars(&p),
            vec![
                ("ICP_CLI_ENVIRONMENT".to_owned(), "production".to_owned()),
                ("ICP_CLI_NETWORK".to_owned(), "ic".to_owned()),
                ("ICP_CLI_CID".to_owned(), principal(1).to_text()),
            ]
        );
    }

    /// Each known canister gets an `ICP_CLI_CID_<NAME>` variable, with the name
    /// uppercased and every non-alphanumeric byte replaced by `_` so the result
    /// is a legal shell identifier.
    #[test]
    fn canister_names_are_normalized_into_env_var_keys() {
        let p = params(&[("my-frontend", principal(2)), ("dep:api", principal(3))]);
        let keys: Vec<String> = system_env_vars(&p)
            .into_iter()
            .map(|(k, _)| k)
            .filter(|k| k.starts_with("ICP_CLI_CID_"))
            .collect();
        // `canister_ids` is a BTreeMap, so ordering follows the canister names.
        assert_eq!(keys, vec!["ICP_CLI_CID_DEP_API", "ICP_CLI_CID_MY_FRONTEND"]);
    }

    /// Resolution takes the commands and cwd from the step and its canister, and
    /// runs nothing.
    #[test]
    fn invocation_resolves_commands_and_cwd() {
        let adapter = Adapter {
            command: CommandField::Commands(vec!["first".to_owned(), "second".to_owned()]),
        };
        let invocation = ScriptInvocation::new(&adapter, &params(&[]));

        assert_eq!(invocation.commands, vec!["first", "second"]);
        assert_eq!(invocation.cwd, PathBuf::from("/work/backend"));
        assert_eq!(invocation.env, system_env_vars(&params(&[])));
    }

    /// The host runner passes the resolved environment through to the subprocess.
    #[tokio::test]
    async fn host_runner_applies_the_resolved_environment() {
        let out = camino_tempfile::NamedUtf8TempFile::new().unwrap();
        let invocation = ScriptInvocation {
            commands: vec![format!("printenv ICP_CLI_NETWORK > '{}'", out.path())],
            cwd: "/".into(),
            env: system_env_vars(&params(&[])),
        };

        HostScripts.run_script(invocation, None).await.unwrap();

        assert_eq!(std::fs::read_to_string(out.path()).unwrap(), "ic\n");
    }

    /// `env` is an overlay, not the whole environment: the script still sees
    /// variables the parent process had. Pins the contract documented on
    /// [`ScriptInvocation::env`].
    ///
    /// Deliberately avoids two things that are not portable across the shells
    /// this runs under. `printenv` accepts only one operand on BSD (macOS) and
    /// so silently drops later names, hence one `echo` — a shell builtin
    /// everywhere — per variable. And the inherited variable is one this test
    /// sets rather than `PATH`, because Git-for-Windows bash rewrites `PATH`
    /// into POSIX form, so its value there never equals the `PATH` the Rust side
    /// reads.
    #[tokio::test]
    async fn host_runner_overlays_rather_than_replaces_the_environment() {
        const AMBIENT: &str = "ICP_CLI_TEST_AMBIENT_VAR";
        const AMBIENT_VALUE: &str = "inherited-from-parent";

        let _guard = ENV_MUTEX.lock().await;
        // SAFETY: ENV_MUTEX serializes the tests in this module that mutate the
        // process environment, and the name is used by this test alone.
        unsafe { std::env::set_var(AMBIENT, AMBIENT_VALUE) };

        let overlaid = camino_tempfile::NamedUtf8TempFile::new().unwrap();
        let inherited = camino_tempfile::NamedUtf8TempFile::new().unwrap();
        let invocation = ScriptInvocation {
            commands: vec![
                format!("echo \"$ICP_CLI_NETWORK\" > '{}'", overlaid.path()),
                format!("echo \"${AMBIENT}\" > '{}'", inherited.path()),
            ],
            cwd: "/".into(),
            env: system_env_vars(&params(&[])),
        };

        let run = HostScripts.run_script(invocation, None).await;

        // SAFETY: as above; the guard is still held.
        unsafe { std::env::remove_var(AMBIENT) };
        run.expect("script must run");

        assert_eq!(
            std::fs::read_to_string(overlaid.path()).unwrap().trim(),
            "ic",
            "the overlaid variable must be set"
        );
        assert_eq!(
            std::fs::read_to_string(inherited.path()).unwrap().trim(),
            AMBIENT_VALUE,
            "a variable the parent process had must still reach the script"
        );
    }

    /// A command that exits non-zero surfaces as a `ScriptRunError` whose source
    /// still names the command and its status, so the `caused by:` line the CLI
    /// prints stays specific.
    #[tokio::test]
    async fn host_runner_reports_a_failing_command() {
        let invocation = ScriptInvocation {
            commands: vec!["exit 3".to_owned()],
            cwd: "/".into(),
            env: vec![],
        };

        let err = HostScripts
            .run_script(invocation, None)
            .await
            .expect_err("a non-zero exit must fail the step");

        assert_eq!(err.to_string(), "script sync step failed");
        assert_eq!(
            std::error::Error::source(&err).expect("cause").to_string(),
            "command 'exit 3' failed with status code 3"
        );
    }
}
