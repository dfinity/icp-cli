// Host-side Component Model runtime for sync plugins.
use std::sync::Arc;

use camino::Utf8PathBuf;
use candid::{Encode, Principal};
use ic_agent::Agent;
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::{DirPerms, FilePerms};

wasmtime::component::bindgen!({
    world: "sync-plugin",
    path: "sync-plugin.wit",
});

use icp::sync_plugin::types::CallType;

// HostState holds everything the plugin's import functions need.
struct HostState {
    target_canister_id: Principal,
    agent: Arc<Agent>,
    /// Proxy canister to route update calls through, if configured.
    proxy: Option<Principal>,
    // WASI context. Preopened directories in this context are the only
    // filesystem locations the plugin can access.
    wasi_ctx: wasmtime_wasi::WasiCtx,
    wasi_table: wasmtime_wasi::ResourceTable,
}

impl wasmtime_wasi::WasiView for HostState {
    fn ctx(&mut self) -> wasmtime_wasi::WasiCtxView<'_> {
        wasmtime_wasi::WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.wasi_table,
        }
    }
}

// `types::Host` is an empty marker trait generated for the `types` interface.
impl icp::sync_plugin::types::Host for HostState {}

impl SyncPluginImports for HostState {
    fn canister_call(&mut self, req: CanisterCallRequest) -> Result<Vec<u8>, String> {
        use icp_canister_interfaces::proxy::{ProxyArgs, ProxyResult};

        let arg_bytes = req.arg;
        let cid = self.target_canister_id;
        let method = req.method.clone();
        let agent = Arc::clone(&self.agent);
        let call_type = req.call_type.unwrap_or(CallType::Update);
        let proxy = if req.direct { None } else { self.proxy };

        // We are already inside tokio::task::block_in_place (see sync/plugin.rs),
        // so blocking the thread here is safe.
        tokio::runtime::Handle::current().block_on(async move {
            match call_type {
                CallType::Update => {
                    if let Some(proxy_cid) = proxy {
                        let proxy_args = ProxyArgs {
                            canister_id: cid,
                            method: method.clone(),
                            args: arg_bytes,
                            cycles: candid::Nat::from(0u64),
                        };
                        let encoded = Encode!(&proxy_args)
                            .map_err(|e| format!("proxy encode failed: {e}"))?;
                        let raw = agent
                            .update(&proxy_cid, "proxy")
                            .with_arg(encoded)
                            .await
                            .map_err(|e| format!("proxy call failed: {e}"))?;
                        let (result,): (ProxyResult,) = candid::decode_args(&raw)
                            .map_err(|e| format!("proxy decode failed: {e}"))?;
                        match result {
                            ProxyResult::Ok(ok) => Ok(ok.result),
                            ProxyResult::Err(err) => Err(err.format_error()),
                        }
                    } else {
                        agent
                            .update(&cid, &method)
                            .with_arg(arg_bytes)
                            .await
                            .map_err(|e| format!("canister call failed: {e}"))
                    }
                }
                CallType::Query => agent
                    .query(&cid, &method)
                    .with_arg(arg_bytes)
                    .call()
                    .await
                    .map_err(|e| format!("canister call failed: {e}")),
            }
        })
    }
}

#[derive(Debug, Snafu)]
pub enum RunPluginError {
    #[snafu(display("failed to create wasmtime engine for plugin at {path}"))]
    CreateEngine {
        source: anyhow::Error,
        path: Utf8PathBuf,
    },

    #[snafu(display("failed to load wasm component from {path}"))]
    LoadComponent {
        source: anyhow::Error,
        path: Utf8PathBuf,
    },

    #[snafu(display("failed to preopen directory '{dir}' for the plugin"))]
    PreopenDir {
        source: anyhow::Error,
        dir: Utf8PathBuf,
    },

    #[snafu(display("failed to instantiate wasm component at {path}"))]
    Instantiate {
        source: anyhow::Error,
        path: Utf8PathBuf,
    },

    #[snafu(display("failed to call exec() on plugin at {path}"))]
    CallExec {
        source: anyhow::Error,
        path: Utf8PathBuf,
    },

    #[snafu(display("plugin returned error: {message}"))]
    PluginFailed { message: String },
}

pub fn run_plugin(
    wasm_path: Utf8PathBuf,
    base_dir: Utf8PathBuf,
    dirs: Vec<String>,
    files: Vec<(String, String)>,
    target_canister_id: Principal,
    agent: Agent,
    proxy: Option<Principal>,
    environment: String,
    stdio: Option<Sender<String>>,
) -> Result<(), RunPluginError> {
    use wasmtime::component::{Component, Linker};
    use wasmtime::{Config, Engine, Store};

    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config).context(CreateEngineSnafu {
        path: wasm_path.clone(),
    })?;

    let component =
        Component::from_file(&engine, wasm_path.as_std_path()).context(LoadComponentSnafu {
            path: wasm_path.clone(),
        })?;

    // Preopen each declared directory read-only. The guest sees it at the
    // same relative path it used in the manifest.
    let mut wasi_builder = wasmtime_wasi::WasiCtxBuilder::new();
    for dir in &dirs {
        let host_path = base_dir.join(dir);
        wasi_builder
            .preopened_dir(
                host_path.as_std_path(),
                dir,
                DirPerms::READ,
                FilePerms::READ,
            )
            .context(PreopenDirSnafu { dir: host_path })?;
    }

    let stdout_pipe = MemoryOutputPipe::new(usize::MAX);
    let stderr_pipe = MemoryOutputPipe::new(usize::MAX);
    if stdio.is_some() {
        wasi_builder
            .stdout(stdout_pipe.clone())
            .stderr(stderr_pipe.clone());
    }

    let host_state = HostState {
        target_canister_id,
        agent: Arc::new(agent),
        proxy,
        wasi_ctx: wasi_builder.build(),
        wasi_table: wasmtime_wasi::ResourceTable::new(),
    };

    let mut linker: Linker<HostState> = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker).context(InstantiateSnafu {
        path: wasm_path.clone(),
    })?;
    SyncPlugin::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |s| s).context(
        InstantiateSnafu {
            path: wasm_path.clone(),
        },
    )?;

    let mut store = Store::new(&engine, host_state);

    let plugin =
        SyncPlugin::instantiate(&mut store, &component, &linker).context(InstantiateSnafu {
            path: wasm_path.clone(),
        })?;

    let input = SyncExecInput {
        canister_id: target_canister_id.to_text(),
        environment,
        dirs,
        files: files
            .into_iter()
            .map(|(name, content)| FileInput { name, content })
            .collect(),
    };

    let result = plugin
        .call_exec(&mut store, &input)
        .context(CallExecSnafu { path: wasm_path })?;

    if let Some(tx) = &stdio {
        for bytes in [stdout_pipe.contents(), stderr_pipe.contents()] {
            if !bytes.is_empty() {
                let s = String::from_utf8_lossy(&bytes).into_owned();
                let _ = tx.blocking_send(s);
            }
        }
    }

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
