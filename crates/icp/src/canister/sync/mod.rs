use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use candid::Principal;
use ic_agent::Agent;
use icp_events::StepReporter;
use snafu::prelude::*;

use crate::manifest::canister::SyncStep;
use crate::package::PackageCache;
use crate::prelude::*;

mod plugin;
pub mod script;

use script::{HostScripts, ScriptInvocation, ScriptRunError, ScriptRunner};

pub struct Params {
    pub path: PathBuf,
    pub cid: Principal,
    /// Name of the environment being synced (e.g. "local", "production").
    /// Passed to sync plugin steps via `SyncExecInput`.
    pub environment: String,
    /// Name of the network (e.g. "local", "ic").
    pub network: String,
    /// IDs of all named canisters in the project for this environment.
    pub canister_ids: BTreeMap<String, Principal>,
    /// Proxy canister to route calls through, if `--proxy` was passed.
    pub proxy: Option<Principal>,
}

#[derive(Debug, Snafu)]
pub enum SynchronizeError {
    #[snafu(transparent)]
    Script { source: ScriptRunError },

    #[snafu(transparent)]
    Plugin { source: plugin::PluginError },
}

#[async_trait]
pub trait Synchronize: Sync + Send {
    async fn sync(
        &self,
        step: &SyncStep,
        params: &Params,
        agent: &Agent,
        reporter: &StepReporter,
        pkg_cache: &PackageCache,
    ) -> Result<Vec<String>, SynchronizeError>;
}

/// Dispatches each sync step to the machinery that runs it. Plugin steps run in
/// the wasmtime WASI sandbox, which this drives directly; script steps go through
/// an injected [`ScriptRunner`], since spawning a subprocess is not available
/// everywhere.
pub struct Syncer {
    scripts: Arc<dyn ScriptRunner>,
}

impl Syncer {
    /// A syncer that runs script steps as host subprocesses.
    pub fn host() -> Self {
        Self::new(Arc::new(HostScripts))
    }

    pub fn new(scripts: Arc<dyn ScriptRunner>) -> Self {
        Self { scripts }
    }
}

#[async_trait]
impl Synchronize for Syncer {
    async fn sync(
        &self,
        step: &SyncStep,
        params: &Params,
        agent: &Agent,
        reporter: &StepReporter,
        pkg_cache: &PackageCache,
    ) -> Result<Vec<String>, SynchronizeError> {
        match step {
            SyncStep::Script(adapter) => Ok(self
                .scripts
                .run_script(ScriptInvocation::new(adapter, params), reporter)
                .await?),
            SyncStep::Plugin(adapter) => Ok(plugin::sync(
                adapter,
                params,
                agent,
                &params.environment,
                params.proxy,
                reporter,
                pkg_cache,
            )
            .await?),
        }
    }
}

#[cfg(test)]
/// Unimplemented mock implementation of `Synchronize`.
/// All methods panic with `unimplemented!()` when called.
pub struct UnimplementedMockSyncer;

#[cfg(test)]
#[async_trait]
impl Synchronize for UnimplementedMockSyncer {
    async fn sync(
        &self,
        _step: &SyncStep,
        _params: &Params,
        _agent: &Agent,
        _reporter: &StepReporter,
        _pkg_cache: &PackageCache,
    ) -> Result<Vec<String>, SynchronizeError> {
        unimplemented!("UnimplementedMockSyncer::sync")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crate::manifest::adapter::script::{Adapter, CommandField};

    use super::*;

    /// A [`ScriptRunner`] that records what it was asked to run instead of
    /// running it, so step dispatch can be tested without spawning a shell.
    #[derive(Default)]
    struct RecordingScripts {
        seen: Mutex<Vec<ScriptInvocation>>,
    }

    #[async_trait]
    impl ScriptRunner for RecordingScripts {
        async fn run_script(
            &self,
            invocation: ScriptInvocation,
            _reporter: &StepReporter,
        ) -> Result<Vec<String>, ScriptRunError> {
            self.seen.lock().unwrap().push(invocation);
            Ok(vec![])
        }
    }

    fn dummy_agent() -> Agent {
        Agent::builder()
            .with_url("http://127.0.0.1:4943")
            .build()
            .expect("build test agent")
    }

    /// A script step reaches the injected runner fully resolved: the commands
    /// from the manifest, the canister directory as cwd, and the `ICP_CLI_*`
    /// environment assembled from the sync params. Nothing is spawned.
    #[tokio::test]
    async fn script_steps_are_dispatched_to_the_injected_runner() {
        let scripts = Arc::new(RecordingScripts::default());
        let syncer = Syncer::new(scripts.clone());

        let cid = Principal::from_slice(&[7; 4]);
        let params = Params {
            path: "/work/backend".into(),
            cid,
            environment: "production".to_owned(),
            network: "ic".to_owned(),
            canister_ids: BTreeMap::from([(
                "my-frontend".to_owned(),
                Principal::from_slice(&[8; 4]),
            )]),
            proxy: None,
        };
        let step = SyncStep::Script(Adapter {
            command: CommandField::Command("./deploy.sh".to_owned()),
        });

        let tmp = camino_tempfile::Utf8TempDir::new().unwrap();
        let pkg_cache = PackageCache::new(tmp.path().to_owned()).unwrap();

        let retained = syncer
            .sync(
                &step,
                &params,
                &dummy_agent(),
                &StepReporter::null(),
                &pkg_cache,
            )
            .await
            .expect("script step should dispatch");
        assert!(retained.is_empty());

        let seen = scripts.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].commands, vec!["./deploy.sh"]);
        assert_eq!(seen[0].cwd, PathBuf::from("/work/backend"));
        assert_eq!(
            seen[0].env,
            vec![
                ("ICP_CLI_ENVIRONMENT".to_owned(), "production".to_owned()),
                ("ICP_CLI_NETWORK".to_owned(), "ic".to_owned()),
                ("ICP_CLI_CID".to_owned(), cid.to_text()),
                (
                    "ICP_CLI_CID_MY_FRONTEND".to_owned(),
                    Principal::from_slice(&[8; 4]).to_text()
                ),
            ]
        );
    }
}
