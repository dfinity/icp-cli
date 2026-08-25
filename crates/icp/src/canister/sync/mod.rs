use async_trait::async_trait;
use ic_agent::Agent;
use icp_deploy_canister::canister::recipe::RemoteResourceResolve;
use icp_deploy_canister::sync_exec::{PluginInvocation, ScriptInvocation};
use icp_events::StepReporter;
use snafu::prelude::*;

mod plugin;

#[derive(Debug, Snafu)]
pub enum SynchronizeError {
    #[snafu(transparent)]
    Script { source: super::script::ScriptError },

    #[snafu(transparent)]
    Plugin { source: plugin::PluginError },
}

/// Host execution of the two sync-step mechanisms that can't run inside a
/// canister: WASI plugins (wasmtime) and subprocess scripts.
///
/// Step dispatch and *all* input derivation (plugin dirs/files, the `ICP_CLI_*`
/// script environment) live in `icp-deploy-canister`; implementations here
/// receive a fully-resolved [`PluginInvocation`] / [`ScriptInvocation`] and
/// perform only the irreducible host action. This trait is the injection seam
/// the [`crate::context::Context`] carries so tests can stub it out.
#[async_trait]
pub trait Synchronize: Sync + Send {
    async fn run_plugin(
        &self,
        invocation: &PluginInvocation,
        agent: &Agent,
        reporter: &StepReporter,
        resolver: &dyn RemoteResourceResolve,
    ) -> Result<Vec<String>, SynchronizeError>;

    async fn run_script(
        &self,
        invocation: &ScriptInvocation,
        reporter: &StepReporter,
    ) -> Result<Vec<String>, SynchronizeError>;
}

pub struct Syncer;

#[async_trait]
impl Synchronize for Syncer {
    async fn run_plugin(
        &self,
        invocation: &PluginInvocation,
        agent: &Agent,
        reporter: &StepReporter,
        resolver: &dyn RemoteResourceResolve,
    ) -> Result<Vec<String>, SynchronizeError> {
        Ok(plugin::run(invocation, agent, reporter, resolver).await?)
    }

    async fn run_script(
        &self,
        invocation: &ScriptInvocation,
        reporter: &StepReporter,
    ) -> Result<Vec<String>, SynchronizeError> {
        let env_refs: Vec<(&str, &str)> = invocation
            .env
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        super::script::execute_commands(&invocation.commands, &invocation.cwd, &env_refs, reporter)
            .await?;
        // Persistent stderr is a sync-plugin feature only; script steps don't
        // currently retain any output past the rolling step view.
        Ok(vec![])
    }
}

#[cfg(test)]
/// Unimplemented mock implementation of `Synchronize`.
/// All methods panic with `unimplemented!()` when called.
pub struct UnimplementedMockSyncer;

#[cfg(test)]
#[async_trait]
impl Synchronize for UnimplementedMockSyncer {
    async fn run_plugin(
        &self,
        _invocation: &PluginInvocation,
        _agent: &Agent,
        _reporter: &StepReporter,
        _resolver: &dyn RemoteResourceResolve,
    ) -> Result<Vec<String>, SynchronizeError> {
        unimplemented!("UnimplementedMockSyncer::run_plugin")
    }

    async fn run_script(
        &self,
        _invocation: &ScriptInvocation,
        _reporter: &StepReporter,
    ) -> Result<Vec<String>, SynchronizeError> {
        unimplemented!("UnimplementedMockSyncer::run_script")
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::Mutex;

    use super::*;

    /// Serializes the tests here that mutate the process environment, since
    /// cargo runs tests in parallel threads. Async-aware because the variable
    /// has to stay set across the subprocess `await` that reads it.
    static ENV_MUTEX: Mutex<()> = Mutex::const_new(());

    fn invocation(commands: Vec<String>, env: Vec<(String, String)>) -> ScriptInvocation {
        ScriptInvocation {
            commands,
            cwd: "/".into(),
            env,
        }
    }

    fn network_env() -> Vec<(String, String)> {
        vec![("ICP_CLI_NETWORK".to_owned(), "ic".to_owned())]
    }

    /// The host runner passes the resolved environment through to the subprocess.
    #[tokio::test]
    async fn host_runner_applies_the_resolved_environment() {
        let out = camino_tempfile::NamedUtf8TempFile::new().unwrap();
        let invocation = invocation(
            vec![format!("printenv ICP_CLI_NETWORK > '{}'", out.path())],
            network_env(),
        );

        Syncer
            .run_script(&invocation, &StepReporter::null())
            .await
            .unwrap();

        assert_eq!(std::fs::read_to_string(out.path()).unwrap(), "ic\n");
    }

    /// A step's `env` is an overlay, not the whole environment: the script still
    /// sees variables the parent process had. Pins the contract documented on
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
        let invocation = invocation(
            vec![
                format!("echo \"$ICP_CLI_NETWORK\" > '{}'", overlaid.path()),
                format!("echo \"${AMBIENT}\" > '{}'", inherited.path()),
            ],
            network_env(),
        );

        let run = Syncer.run_script(&invocation, &StepReporter::null()).await;

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

    /// A command that exits non-zero surfaces as a `SynchronizeError` whose
    /// source still names the command and its status, so the `caused by:` line
    /// the CLI prints stays specific.
    #[tokio::test]
    async fn host_runner_reports_a_failing_command() {
        let err = Syncer
            .run_script(
                &invocation(vec!["exit 3".to_owned()], vec![]),
                &StepReporter::null(),
            )
            .await
            .expect_err("a non-zero exit must fail the step");

        assert_eq!(
            err.to_string(),
            "command 'exit 3' failed with status code 3"
        );
    }
}
