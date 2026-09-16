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

#[cfg(feature = "host")]
use script::HostScripts;
use script::{ScriptInvocation, ScriptRunError, ScriptRunner};

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
    #[cfg(feature = "host")]
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

    use indexmap::IndexMap;

    use crate::manifest::adapter::plugin::{Adapter as PluginAdapter, NamedPaths, PathOrList};
    use crate::manifest::adapter::prebuilt::{LocalSource, SourceField};
    use crate::manifest::adapter::script::{Adapter, CommandField};

    use super::*;

    use plugin::{Invocation, KeyedPath, RunError};

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

    /// A [`plugin::Run`] that records the invocation it was handed instead of
    /// loading a wasm component, and reports stderr lines the way a plugin that
    /// retained some would.
    struct RecordingRun {
        seen: Mutex<Vec<Invocation>>,
        retained: Vec<String>,
    }

    #[async_trait]
    impl plugin::Run for RecordingRun {
        async fn run(&self, invocation: Invocation) -> Result<Vec<String>, RunError> {
            self.seen.lock().unwrap().push(invocation);
            Ok(self.retained.clone())
        }
    }

    /// A component runtime's own error, standing in for the kind of error this
    /// crate cannot name — the whole reason [`RunError`] carries its cause
    /// boxed.
    #[derive(Debug, Snafu)]
    #[snafu(display("the component runtime gave up"))]
    struct RuntimeGaveUp;

    /// A [`plugin::Run`] that fails the way a component runtime does.
    struct FailingRun;

    #[async_trait]
    impl plugin::Run for FailingRun {
        async fn run(&self, _invocation: Invocation) -> Result<Vec<String>, RunError> {
            Err(RunError::new(RuntimeGaveUp))
        }
    }

    /// A [`wasm::Fetch`] that reports a fixed path for the module and records
    /// what it was asked to resolve, so the step's declared source can be
    /// checked without a wasm file on disk.
    struct StubFetch {
        path: PathBuf,
        asked: Mutex<Vec<(SourceField, PathBuf, Option<String>)>>,
    }

    impl StubFetch {
        fn new(path: &str) -> Self {
            Self {
                path: path.into(),
                asked: Mutex::default(),
            }
        }
    }

    #[async_trait]
    impl wasm::Fetch for StubFetch {
        async fn wasm(
            &self,
            source: &SourceField,
            base_dir: &Path,
            sha256: Option<&str>,
            _reporter: &StepReporter,
        ) -> Result<PathBuf, wasm::FetchError> {
            self.asked.lock().unwrap().push((
                source.clone(),
                base_dir.to_path_buf(),
                sha256.map(str::to_owned),
            ));
            Ok(self.path.clone())
        }
    }

    fn principal(byte: u8) -> Principal {
        Principal::from_slice(&[byte; 4])
    }

    fn params() -> Params {
        Params {
            path: "/work/backend".into(),
            project_dir: "/work".into(),
            cid: principal(7),
            name: "backend".to_owned(),
            environment: "production".to_owned(),
            network: "ic".to_owned(),
            urls: NetworkUrls {
                api_url: "https://icp-api.io".parse().expect("valid api url"),
                http_gateway_url: Some("https://icp0.io".parse().expect("valid gateway url")),
            },
            canister_ids: BTreeMap::from([("my-frontend".to_owned(), principal(8))]),
            proxy: None,
        }
    }

    /// A plugin step declaring a key holding one path, a key holding two, a
    /// field, and whatever `canisters:` list the test needs. Which of the
    /// declared paths are directories is the runner's to work out from disk, so
    /// nothing here has to exist.
    fn plugin_step(canisters: Option<Vec<String>>) -> SyncStep {
        SyncStep::Plugin(Box::new(PluginAdapter {
            source: SourceField::Local(LocalSource {
                path: "plugins/seed.wasm".into(),
            }),
            sha256: Some("abc123".to_owned()),
            dirs: None,
            files: Some(NamedPaths::Map(IndexMap::from([
                ("seed".to_owned(), PathOrList::One("seed-data".to_owned())),
                (
                    "config".to_owned(),
                    PathOrList::Many(vec!["a.txt".to_owned(), "b.txt".to_owned()]),
                ),
            ]))),
            fields: Some(BTreeMap::from([("greeting".to_owned(), "hi".to_owned())])),
            canisters,
        }))
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

        let params = params();
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
                ("ICP_CLI_CID".to_owned(), principal(7).to_text()),
                ("ICP_CLI_CID_MY_FRONTEND".to_owned(), principal(8).to_text()),
            ]
        );
    }

    /// A plugin step reaches the injected runner fully resolved: the wasm the
    /// fetch seam reported, the declared paths tagged with the keys they were
    /// written under, the canister ID table as the plugin will see it, and the
    /// step's `canisters:` list resolved against that table. The stderr lines
    /// the runner reports come back to the caller.
    #[tokio::test]
    async fn plugin_steps_are_dispatched_to_the_injected_runner() {
        let scripts = Arc::new(RecordingScripts::default());
        let wasm = Arc::new(StubFetch::new("/cache/seed.wasm"));
        let plugins = Arc::new(RecordingRun {
            seen: Mutex::default(),
            retained: vec!["registered 3 fruits".to_owned()],
        });
        let syncer = Syncer::new(scripts.clone(), wasm.clone(), plugins.clone());
        let calls: Arc<dyn CanisterCalls> = Arc::new(crate::calls::UnimplementedMockCalls);

        // A canister in a subproject, so the table the plugin sees is the
        // resolved one rather than the params' own keys.
        let sibling = principal(3);
        let mut params = params();
        let cid = params.cid;
        params.name = "services/crm:backend".to_owned();
        params.proxy = Some(principal(9));
        params.canister_ids = BTreeMap::from([
            ("services/crm:backend".to_owned(), cid),
            ("services/crm:frontend".to_owned(), sibling),
        ]);

        let step = plugin_step(Some(vec!["frontend".to_owned()]));
        let retained = syncer
            .sync(&step, &params, &calls, &StepReporter::null())
            .await
            .expect("plugin step should dispatch");
        assert_eq!(retained, ["registered 3 fruits"]);
        // The step went to the plugin runner alone.
        assert!(scripts.seen.lock().unwrap().is_empty());

        // The module was asked for by the step's own source, checksum and
        // canister directory.
        let asked = wasm.asked.lock().unwrap();
        assert_eq!(
            &asked[..],
            [(
                SourceField::Local(LocalSource {
                    path: "plugins/seed.wasm".into()
                }),
                params.path.clone(),
                Some("abc123".to_owned()),
            )]
        );

        let seen = plugins.seen.lock().unwrap();
        let [invocation] = &seen[..] else {
            panic!("expected exactly one invocation, got {}", seen.len());
        };
        assert_eq!(invocation.wasm_path, PathBuf::from("/cache/seed.wasm"));
        assert_eq!(invocation.base_dir, params.path);
        assert_eq!(invocation.project_dir, params.project_dir);
        assert!(invocation.dirs.is_empty());
        // Every declared path arrives in written order under the key it was
        // written beneath, a key holding a list repeating across its paths.
        assert_eq!(
            invocation.files,
            [
                KeyedPath {
                    key: Some("seed".to_owned()),
                    path: "seed-data".to_owned(),
                },
                KeyedPath {
                    key: Some("config".to_owned()),
                    path: "a.txt".to_owned(),
                },
                KeyedPath {
                    key: Some("config".to_owned()),
                    path: "b.txt".to_owned(),
                },
            ]
        );
        assert_eq!(
            invocation.fields,
            BTreeMap::from([("greeting".to_owned(), "hi".to_owned())])
        );
        assert_eq!(invocation.host_canister_id, cid);
        assert_eq!(invocation.proxy, params.proxy);
        assert_eq!(invocation.environment, params.environment);
        assert_eq!(invocation.api_url, params.urls.api_url);
        assert_eq!(invocation.gateway_url, params.urls.http_gateway_url);
        assert_eq!(
            invocation.compute_limit_secs,
            plugin::DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS
        );
        // The syncing canister's own subproject names its siblings by their
        // bare local names...
        assert_eq!(invocation.canister_ids.get("frontend"), Some(&sibling));
        // ...which is the table the `canisters:` list is resolved against.
        assert_eq!(
            invocation.callable.by_name,
            BTreeMap::from([("frontend".to_owned(), sibling)])
        );
        // The plugin's calls go through the very seam the caller passed in.
        assert!(Arc::ptr_eq(&calls, &invocation.calls));
    }

    /// The runner's own failure is what the caller sees: the seam names the
    /// action it was attempting and passes the cause through rather than
    /// restating it.
    #[tokio::test]
    async fn a_failing_plugin_runner_surfaces_its_own_cause() {
        let syncer = Syncer::new(
            Arc::new(RecordingScripts::default()),
            Arc::new(StubFetch::new("/cache/seed.wasm")),
            Arc::new(FailingRun),
        );
        let calls: Arc<dyn CanisterCalls> = Arc::new(crate::calls::UnimplementedMockCalls);

        let err = syncer
            .sync(&plugin_step(None), &params(), &calls, &StepReporter::null())
            .await
            .expect_err("a failing runner must fail the step");
        assert!(
            matches!(
                err,
                SynchronizeError::Plugin {
                    source: plugin::PluginError::RunPlugin { .. }
                }
            ),
            "unexpected error: {err}"
        );
        let rendered = crate::error::flatten(&err);
        assert!(rendered.contains("failed to run plugin"), "got: {rendered}");
        assert_eq!(
            rendered.matches("the component runtime gave up").count(),
            1,
            "the runtime's own message should be reported once: {rendered}"
        );
    }
}
