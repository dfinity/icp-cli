use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use candid::Principal;
use icp_events::StepReporter;
use snafu::prelude::*;

use crate::calls::CanisterCalls;
use crate::canister::wasm;
use crate::manifest::canister::SyncStep;
use crate::network::NetworkUrls;
use crate::prelude::*;

pub mod declared;
pub mod plugin;
pub mod script;

use script::{HostScripts, ScriptInvocation, ScriptRunError, ScriptRunner};

pub struct Params {
    pub path: PathBuf,
    /// The project (workspace root) directory. It bounds what a sync plugin may
    /// read: a declared `dirs`/`files` entry may rise out of the canister
    /// directory into the rest of the project, but not out of the project.
    pub project_dir: PathBuf,
    pub cid: Principal,
    /// Fully-qualified store key of the canister being synced (e.g. `backend`,
    /// or `services/open-crm:backend` for a canister in a subproject). Its namespace
    /// prefix identifies which other canisters are in the same subproject.
    pub name: String,
    /// Name of the environment being synced (e.g. "local", "production").
    /// Passed to sync plugin steps via `SyncExecInput`.
    pub environment: String,
    /// Name of the network (e.g. "local", "ic").
    pub network: String,
    /// The network's API endpoint, where canister calls are submitted, and its
    /// HTTP gateway if it exposes one. Passed to sync plugin steps via
    /// `SyncExecInput`.
    pub urls: NetworkUrls,
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
        calls: &Arc<dyn CanisterCalls>,
        reporter: &StepReporter,
    ) -> Result<Vec<String>, SynchronizeError>;
}

/// Dispatches each sync step to the machinery that runs it. Neither kind can be
/// run from here: a script step needs a subprocess and a plugin step needs a
/// wasm component runtime, so each goes through an injected runner.
pub struct Syncer {
    scripts: Arc<dyn ScriptRunner>,
    wasm: Arc<dyn wasm::Fetch>,
    plugins: Arc<dyn plugin::Run>,
}

impl Syncer {
    /// A syncer that runs script steps as host subprocesses.
    pub fn host(wasm: Arc<dyn wasm::Fetch>, plugins: Arc<dyn plugin::Run>) -> Self {
        Self::new(Arc::new(HostScripts), wasm, plugins)
    }

    pub fn new(
        scripts: Arc<dyn ScriptRunner>,
        wasm: Arc<dyn wasm::Fetch>,
        plugins: Arc<dyn plugin::Run>,
    ) -> Self {
        Self {
            scripts,
            wasm,
            plugins,
        }
    }
}

#[async_trait]
impl Synchronize for Syncer {
    async fn sync(
        &self,
        step: &SyncStep,
        params: &Params,
        calls: &Arc<dyn CanisterCalls>,
        reporter: &StepReporter,
    ) -> Result<Vec<String>, SynchronizeError> {
        match step {
            SyncStep::Script(adapter) => Ok(self
                .scripts
                .run_script(ScriptInvocation::new(adapter, params), reporter)
                .await?),
            SyncStep::Plugin(adapter) => Ok(plugin::sync(
                adapter,
                params,
                calls,
                reporter,
                self.wasm.as_ref(),
                self.plugins.as_ref(),
            )
            .await?),
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
/// Unimplemented mock implementation of `Synchronize`.
/// All methods panic with `unimplemented!()` when called.
pub struct UnimplementedMockSyncer;

#[cfg(any(test, feature = "test-util"))]
#[async_trait]
impl Synchronize for UnimplementedMockSyncer {
    async fn sync(
        &self,
        _step: &SyncStep,
        _params: &Params,
        _calls: &Arc<dyn CanisterCalls>,
        _reporter: &StepReporter,
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

    /// A script step reaches the injected runner fully resolved: the commands
    /// from the manifest, the canister directory as cwd, and the `ICP_CLI_*`
    /// environment assembled from the sync params. Nothing is spawned.
    #[tokio::test]
    async fn script_steps_are_dispatched_to_the_injected_runner() {
        let scripts = Arc::new(RecordingScripts::default());
        let syncer = Syncer::new(
            scripts.clone(),
            Arc::new(wasm::UnimplementedMockFetch),
            Arc::new(plugin::UnimplementedMockRun),
        );
        let calls: Arc<dyn CanisterCalls> = Arc::new(crate::calls::UnimplementedMockCalls);

        let cid = Principal::from_slice(&[7; 4]);
        let params = Params {
            path: "/work/backend".into(),
            project_dir: "/work".into(),
            cid,
            name: "backend".to_owned(),
            environment: "production".to_owned(),
            network: "ic".to_owned(),
            urls: NetworkUrls {
                api_url: "https://icp-api.io".parse().expect("valid api url"),
                http_gateway_url: Some("https://icp0.io".parse().expect("valid gateway url")),
            },
            canister_ids: BTreeMap::from([(
                "my-frontend".to_owned(),
                Principal::from_slice(&[8; 4]),
            )]),
            proxy: None,
        };
        let step = SyncStep::Script(Adapter {
            command: CommandField::Command("./deploy.sh".to_owned()),
        });

        let retained = syncer
            .sync(&step, &params, &calls, &StepReporter::null())
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
