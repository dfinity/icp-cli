// Host-side Component Model runtime for sync plugins.
// The WIT world is in sync-plugin/sync-plugin.wit.

use std::sync::Arc;

use camino::Utf8PathBuf;
use candid::Principal;
use ic_agent::Agent;
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::sandbox::is_path_allowed;

wasmtime::component::bindgen!({
    world: "sync-plugin",
    path: "../../sync-plugin/sync-plugin.wit",
});

use icp::sync_plugin::types::CallType;

// HostState holds everything the plugin's import functions need.
struct HostState {
    target_canister_id: Principal,
    agent: Arc<Agent>,
    allowed_dirs: Arc<Vec<Utf8PathBuf>>,
    base_dir: Arc<Utf8PathBuf>,
    stdio: Option<Sender<String>>,
}

// `types::Host` is an empty marker trait generated for the `types` interface.
impl icp::sync_plugin::types::Host for HostState {}

impl SyncPluginImports for HostState {
    fn canister_call(&mut self, req: CanisterCallRequest) -> Result<String, String> {
        let arg_bytes = candid_parser::parse_idl_args(&req.arg)
            .map_err(|e| format!("failed to parse Candid arg: {e}"))?
            .to_bytes()
            .map_err(|e| format!("failed to encode Candid arg: {e}"))?;

        let cid = self.target_canister_id;
        let method = req.method.clone();
        let agent = Arc::clone(&self.agent);
        let call_type = req.call_type.unwrap_or(CallType::Update);

        // We are already inside tokio::task::block_in_place (see sync/plugin.rs),
        // so blocking the thread here is safe.
        let result = tokio::runtime::Handle::current()
            .block_on(async move {
                match call_type {
                    CallType::Update => agent.update(&cid, &method).with_arg(arg_bytes).await,
                    CallType::Query => {
                        agent
                            .query(&cid, &method)
                            .with_arg(arg_bytes)
                            .call()
                            .await
                    }
                }
            })
            .map_err(|e| format!("canister call failed: {e}"))?;

        candid::IDLArgs::from_bytes(&result)
            .map(|args| args.to_string())
            .map_err(|e| format!("failed to decode canister response: {e}"))
    }

    fn read_file(&mut self, path: String) -> Result<String, String> {
        let full_path = self.base_dir.join(&path);
        let canon_std = std::fs::canonicalize(full_path.as_std_path())
            .map_err(|e| format!("failed to resolve path '{path}': {e}"))?;
        let canon = Utf8PathBuf::from_path_buf(canon_std)
            .map_err(|p| format!("path is not valid UTF-8: {}", p.display()))?;

        if !is_path_allowed(&canon, &self.allowed_dirs) {
            return Err(format!(
                "access denied: '{path}' is outside the declared dirs allowlist"
            ));
        }

        std::fs::read_to_string(canon.as_std_path())
            .map_err(|e| format!("failed to read file '{path}': {e}"))
    }

    fn list_dir(&mut self, path: String) -> Result<Vec<DirEntry>, String> {
        let full_path = self.base_dir.join(&path);
        let canon_std = std::fs::canonicalize(full_path.as_std_path())
            .map_err(|e| format!("failed to resolve path '{path}': {e}"))?;
        let canon = Utf8PathBuf::from_path_buf(canon_std)
            .map_err(|p| format!("path is not valid UTF-8: {}", p.display()))?;

        if !is_path_allowed(&canon, &self.allowed_dirs) {
            return Err(format!(
                "access denied: '{path}' is outside the declared dirs allowlist"
            ));
        }

        std::fs::read_dir(canon.as_std_path())
            .map_err(|e| format!("failed to read directory '{path}': {e}"))?
            .map(|entry| {
                let entry = entry.map_err(|e| format!("failed to read directory entry: {e}"))?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_dir = entry
                    .file_type()
                    .map_err(|e| format!("failed to get file type for '{name}': {e}"))?
                    .is_dir();
                Ok(DirEntry { name, is_dir })
            })
            .collect()
    }

    fn log(&mut self, message: String) {
        if let Some(tx) = &self.stdio {
            let _ = tx.blocking_send(message);
        }
    }
}

#[derive(Debug, Snafu)]
pub enum RunPluginError {
    #[snafu(display("failed to create wasmtime engine for plugin at {path}"))]
    CreateEngine { source: anyhow::Error, path: Utf8PathBuf },

    #[snafu(display("failed to load wasm component from {path}"))]
    LoadComponent { source: anyhow::Error, path: Utf8PathBuf },

    #[snafu(display("failed to instantiate wasm component at {path}"))]
    Instantiate { source: anyhow::Error, path: Utf8PathBuf },

    #[snafu(display("failed to call exec() on plugin at {path}"))]
    CallExec { source: anyhow::Error, path: Utf8PathBuf },

    #[snafu(display("plugin returned error: {message}"))]
    PluginFailed { message: String },
}

pub fn run_plugin(
    wasm_path: Utf8PathBuf,
    base_dir: Utf8PathBuf,
    allowed_dirs: Vec<Utf8PathBuf>,
    target_canister_id: Principal,
    agent: Agent,
    environment: String,
    stdio: Option<Sender<String>>,
) -> Result<(), RunPluginError> {
    use wasmtime::component::{Component, Linker};
    use wasmtime::{Config, Engine, Store};

    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine =
        Engine::new(&config).context(CreateEngineSnafu { path: wasm_path.clone() })?;

    let component = Component::from_file(&engine, wasm_path.as_std_path())
        .context(LoadComponentSnafu { path: wasm_path.clone() })?;

    let canister_id_text = target_canister_id.to_text();

    let host_state = HostState {
        target_canister_id,
        agent: Arc::new(agent),
        allowed_dirs: Arc::new(allowed_dirs),
        base_dir: Arc::new(base_dir),
        stdio,
    };

    let mut linker: Linker<HostState> = Linker::new(&engine);
    SyncPlugin::add_to_linker(&mut linker, |s| s)
        .context(InstantiateSnafu { path: wasm_path.clone() })?;

    let mut store = Store::new(&engine, host_state);

    let plugin = SyncPlugin::instantiate(&mut store, &component, &linker)
        .context(InstantiateSnafu { path: wasm_path.clone() })?;

    let input = SyncExecInput {
        canister_id: canister_id_text,
        environment,
    };

    let result = plugin
        .call_exec(&mut store, &input)
        .context(CallExecSnafu { path: wasm_path })?;

    let stdio = store.into_data().stdio;
    match result {
        Ok(Some(msg)) => {
            if let Some(tx) = &stdio {
                let _ = tx.blocking_send(msg);
            }
        }
        Ok(None) => {}
        Err(message) => return PluginFailedSnafu { message }.fail(),
    }

    Ok(())
}
