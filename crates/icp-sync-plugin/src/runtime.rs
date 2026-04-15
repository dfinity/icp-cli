// Runtime implementation — to be written using wasmtime::component.
// See sync-plugin/sync-plugin.wit for the interface definition.

use camino::Utf8PathBuf;
use candid::Principal;
use ic_agent::Agent;
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

#[derive(Debug, Snafu)]
pub enum RunPluginError {
    #[snafu(display("failed to load wasm component from {path}"))]
    LoadComponent { path: Utf8PathBuf },

    #[snafu(display("failed to call exec() on plugin at {path}"))]
    CallExec { path: Utf8PathBuf },

    #[snafu(display("plugin returned error: {message}"))]
    PluginFailed { message: String },
}

pub fn run_plugin(
    _wasm_path: Utf8PathBuf,
    _base_dir: Utf8PathBuf,
    _allowed_dirs: Vec<Utf8PathBuf>,
    _target_canister_id: Principal,
    _agent: Agent,
    _environment: String,
    _stdio: Option<Sender<String>>,
) -> Result<(), RunPluginError> {
    unimplemented!("sync plugin runtime: migration to wasmtime Component Model in progress")
}
