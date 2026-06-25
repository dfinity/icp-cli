// Host-side Component Model runtime for sync plugins.
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::{Context as TaskContext, Poll};
use std::time::{Duration, Instant};

const MAX_PLUGIN_OUTPUT: usize = 1024 * 1024; // 1 MiB per stream
// Maximum wasm call-stack depth (in bytes).
const MAX_WASM_STACK: usize = 512 * 1024;
// How many seconds of pure wasm compute a plugin may use (host-call latency is excluded).
const PLUGIN_COMPUTE_LIMIT_SECS: u64 = 60;

use bytes::Bytes;
use camino::{Utf8Component, Utf8PathBuf};
use candid::{Encode, Principal};
use ic_agent::Agent;
use snafu::prelude::*;
use tokio::io::{self, AsyncWrite};
use tokio::sync::mpsc::Sender;
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
use wasmtime_wasi::p2::{OutputStream, Pollable, StreamError};
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
    // Accumulated epoch ticks to grant back after a host call returns, so that
    // canister call latency doesn't consume the wasm compute budget. AtomicU64
    // (rather than Mutex<u64>) is required because the epoch_deadline_callback
    // closure must be Send + 'static, which Arc<Cell<u64>> does not satisfy.
    epoch_extension: Arc<AtomicU64>,
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
        let proxy = if req.direct { None } else { self.proxy };

        // We are already inside tokio::task::block_in_place (see sync/plugin.rs),
        // so blocking the thread here is safe.
        let start = Instant::now();
        let result = tokio::runtime::Handle::current().block_on(async move {
            match req.call_type {
                CallType::Update => {
                    if let Some(proxy_cid) = proxy {
                        let proxy_args = ProxyArgs {
                            canister_id: cid,
                            method: method.clone(),
                            args: arg_bytes,
                            cycles: candid::Nat::from(req.cycles),
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
        });
        // Return the time spent in the host call to the compute budget so
        // canister network latency doesn't count against the plugin's limit.
        let elapsed_ticks = start.elapsed().as_secs() + 1;
        self.epoch_extension
            .fetch_add(elapsed_ticks, Ordering::Relaxed);
        result
    }
}

// Used as the error payload inside the epoch_deadline_callback closure, which
// must return wasmtime::Error (= anyhow::Error). Snafu derives std::error::Error
// so .into() converts it via anyhow's blanket From<impl StdError + Send + Sync>.
#[derive(Debug, Snafu)]
#[snafu(display("plugin exceeded the {PLUGIN_COMPUTE_LIMIT_SECS}s compute time limit"))]
struct ComputeTimeLimitExceeded;

#[derive(Debug, Snafu)]
pub enum RunPluginError {
    #[snafu(display("failed to create wasmtime engine for plugin at {path}"))]
    CreateEngine {
        source: wasmtime::Error,
        path: Utf8PathBuf,
    },

    #[snafu(display("failed to load wasm component from {path}"))]
    LoadComponent {
        source: wasmtime::Error,
        path: Utf8PathBuf,
    },

    #[snafu(display(
        "plugin dir '{dir}' is not a safe relative path (no absolute paths or '..' allowed)"
    ))]
    UnsafeDir { dir: String },

    #[snafu(display(
        "plugin dir '{dir}' resolves through a symlink ('{link}'); symlinks are not allowed in plugin dirs"
    ))]
    SymlinkDir { dir: String, link: Utf8PathBuf },

    #[snafu(display("failed to preopen directory '{dir}' for the plugin"))]
    PreopenDir {
        source: wasmtime::Error,
        dir: Utf8PathBuf,
    },

    #[snafu(display("failed to instantiate wasm component at {path}"))]
    Instantiate {
        source: wasmtime::Error,
        path: Utf8PathBuf,
    },

    #[snafu(display("failed to call exec() on plugin at {path}"))]
    CallExec {
        source: wasmtime::Error,
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
    identity_principal: Principal,
    environment: String,
    stdio: Option<Sender<String>>,
) -> Result<Vec<String>, RunPluginError> {
    use wasmtime::component::{Component, Linker};
    use wasmtime::{Config, Engine, Store};

    let mut config = Config::new();
    config.wasm_component_model(true);
    config.max_wasm_stack(MAX_WASM_STACK);
    // Linear memory is implicitly bounded by the wasm32 address space (4 GiB).
    // If wasm64 support is ever added, set Config::memory_maximum() explicitly.
    config.epoch_interruption(true);
    let engine = Engine::new(&config).context(CreateEngineSnafu {
        path: wasm_path.clone(),
    })?;

    // Increment the engine epoch every second from a background thread.
    // The store deadline is set below; the ticker stops when this guard is dropped.
    // AtomicBool is sufficient here — it's a one-way stop signal between two threads.
    let ticker_stop = Arc::new(AtomicBool::new(false));
    let _ticker_guard = {
        let engine_ticker = engine.clone();
        let stop = ticker_stop.clone();
        let handle = std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(1));
                engine_ticker.increment_epoch();
            }
        });
        let _ = handle; // detached; exits within 1 s once stop is set
        // RAII guard: signals the ticker thread to stop when dropped.
        struct TickerGuard(Arc<AtomicBool>);
        impl Drop for TickerGuard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        TickerGuard(ticker_stop)
    };

    let component =
        Component::from_file(&engine, wasm_path.as_std_path()).context(LoadComponentSnafu {
            path: wasm_path.clone(),
        })?;

    // Preopen each declared directory read-only. The guest sees it at the
    // same relative path it used in the manifest.
    let mut wasi_builder = wasmtime_wasi::WasiCtxBuilder::new();
    for dir in &dirs {
        let p = Utf8PathBuf::from(dir);
        ensure!(
            !p.is_absolute() && !p.components().any(|c| c == Utf8Component::ParentDir),
            UnsafeDirSnafu { dir }
        );
        // Reject symlinks in the declared path: neither the final entry nor any
        // intermediate component may be a symlink, so the preopen cannot escape
        // `base_dir` to a target elsewhere on disk. (Symlinks *inside* a preopen
        // that escape it are separately rejected by the WASI sandbox.)
        if let Some(link) = crate::first_symlink_component(&base_dir, dir) {
            return SymlinkDirSnafu { dir, link }.fail();
        }
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

    let persistent_stderr: Arc<StdMutex<Vec<String>>> = Arc::default();
    let stdout_capture = LineCapture::new("stdout", stdio.clone(), None);
    let stderr_capture = LineCapture::new("stderr", stdio.clone(), Some(persistent_stderr.clone()));
    wasi_builder
        .stdout(stdout_capture.clone())
        .stderr(stderr_capture.clone());

    let epoch_extension = Arc::new(AtomicU64::new(0));
    let host_state = HostState {
        target_canister_id,
        agent: Arc::new(agent),
        proxy,
        wasi_ctx: wasi_builder.build(),
        wasi_table: wasmtime_wasi::ResourceTable::new(),
        epoch_extension: epoch_extension.clone(),
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
    store.set_epoch_deadline(PLUGIN_COMPUTE_LIMIT_SECS);
    store.epoch_deadline_callback(move |_| {
        let extra = epoch_extension.swap(0, Ordering::Relaxed);
        if extra > 0 {
            Ok(wasmtime::UpdateDeadline::Continue(extra))
        } else {
            Err(ComputeTimeLimitExceeded.into())
        }
    });

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
        identity_principal: identity_principal.to_text(),
        proxy_canister_id: proxy.map(|p| p.to_text()),
    };

    let call_result = plugin.call_exec(&mut store, &input);

    // Flush any partial line and emit the truncation note (if any) before
    // we hand control back, so the last line of plugin output isn't lost.
    stdout_capture.finalize();
    stderr_capture.finalize();

    match call_result.context(CallExecSnafu { path: wasm_path })? {
        Ok(()) => {}
        Err(message) => return PluginFailedSnafu { message }.fail(),
    }

    let lines = std::mem::take(&mut *persistent_stderr.lock().unwrap());
    Ok(lines)
}

// -------------------------------------------------------------------------
// Plugin stdout/stderr capture
// -------------------------------------------------------------------------
//
// `LineCapture` implements both `StdoutStream` (so it can be installed on a
// `WasiCtxBuilder`) and `OutputStream` / `AsyncWrite` (so the bytes written
// by the guest flow through the same code path). Each write is split on
// newlines; complete lines have ANSI escapes stripped and are pushed to the
// rolling-view `Sender<String>` via `try_send` (best-effort). For stderr,
// the same lines are also appended to `persistent`, which is drained by
// `run_plugin()` after `exec()` returns. Total accepted bytes are capped at
// `MAX_PLUGIN_OUTPUT` per stream; further bytes are dropped and `finalize`
// emits a single "… N bytes of <label> truncated" line.

#[derive(Default)]
struct CaptureState {
    /// Bytes seen since the last newline, awaiting more input or finalize.
    partial: Vec<u8>,
    /// Total bytes accepted (i.e. counted toward the cap).
    bytes_written: usize,
    /// Total bytes dropped after hitting the cap.
    bytes_dropped: usize,
}

#[derive(Clone)]
struct LineCapture {
    state: Arc<StdMutex<CaptureState>>,
    label: &'static str,
    forward: Option<Sender<String>>,
    persistent: Option<Arc<StdMutex<Vec<String>>>>,
}

impl LineCapture {
    fn new(
        label: &'static str,
        forward: Option<Sender<String>>,
        persistent: Option<Arc<StdMutex<Vec<String>>>>,
    ) -> Self {
        Self {
            state: Arc::default(),
            label,
            forward,
            persistent,
        }
    }

    fn push_bytes(&self, buf: &[u8]) {
        let mut to_emit: Vec<String> = Vec::new();
        {
            let mut st = self.state.lock().unwrap();
            let remaining = MAX_PLUGIN_OUTPUT.saturating_sub(st.bytes_written);
            let (accepted, dropped) = if buf.len() > remaining {
                (&buf[..remaining], buf.len() - remaining)
            } else {
                (buf, 0)
            };
            st.bytes_written += accepted.len();
            st.bytes_dropped += dropped;
            st.partial.extend_from_slice(accepted);
            while let Some(pos) = st.partial.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = st.partial.drain(..=pos).collect();
                let s = String::from_utf8_lossy(&line);
                let trimmed = s.trim_end_matches('\n').trim_end_matches('\r');
                to_emit.push(console::strip_ansi_codes(trimmed).into_owned());
            }
        }
        for line in to_emit {
            self.emit(line);
        }
    }

    fn emit(&self, line: String) {
        if let Some(tx) = &self.forward {
            let _ = tx.try_send(line.clone());
        }
        if let Some(p) = &self.persistent {
            p.lock().unwrap().push(line);
        }
    }

    /// Flush any partial line and emit a single truncation note if we dropped
    /// bytes past the cap. Called exactly once, after `exec()` returns.
    fn finalize(&self) {
        let (partial, dropped) = {
            let mut st = self.state.lock().unwrap();
            (std::mem::take(&mut st.partial), st.bytes_dropped)
        };
        if !partial.is_empty() {
            let s = String::from_utf8_lossy(&partial);
            let trimmed = s.trim_end_matches('\n').trim_end_matches('\r');
            if !trimmed.is_empty() {
                let line = console::strip_ansi_codes(trimmed).into_owned();
                self.emit(line);
            }
        }
        if dropped > 0 {
            self.emit(format!("… {dropped} bytes of {} truncated", self.label));
        }
    }
}

impl IsTerminal for LineCapture {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for LineCapture {
    fn p2_stream(&self) -> Box<dyn OutputStream> {
        Box::new(self.clone())
    }
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(self.clone())
    }
}

#[async_trait::async_trait]
impl Pollable for LineCapture {
    async fn ready(&mut self) {}
}

#[async_trait::async_trait]
impl OutputStream for LineCapture {
    fn write(&mut self, bytes: Bytes) -> Result<(), StreamError> {
        self.push_bytes(&bytes);
        Ok(())
    }
    fn flush(&mut self) -> Result<(), StreamError> {
        Ok(())
    }
    fn check_write(&mut self) -> Result<usize, StreamError> {
        Ok(usize::MAX)
    }
}

impl AsyncWrite for LineCapture {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.push_bytes(buf);
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use candid::Principal;
    use ic_agent::Agent;

    fn dummy_agent() -> Agent {
        Agent::builder()
            .with_url("http://127.0.0.1:4943")
            .build()
            .expect("build test agent")
    }

    fn anon() -> Principal {
        Principal::anonymous()
    }

    // -------------------------------------------------------------------------
    // Error-path tests — no fixture WASM needed
    // -------------------------------------------------------------------------

    #[test]
    fn load_component_error_on_missing_file() {
        let result = run_plugin(
            "nonexistent.wasm".into(),
            ".".into(),
            vec![],
            vec![],
            anon(),
            dummy_agent(),
            None,
            anon(),
            "test".to_string(),
            None,
        );
        assert!(matches!(result, Err(RunPluginError::LoadComponent { .. })));
    }

    // -------------------------------------------------------------------------
    // Fixture-dependent tests
    // -------------------------------------------------------------------------

    #[test]
    fn preopen_dir_error_on_missing_dir() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let result = run_plugin(
            wasm_path.into(),
            ".".into(),
            vec!["nonexistent_dir".to_string()],
            vec![],
            anon(),
            dummy_agent(),
            None,
            anon(),
            "test".to_string(),
            None,
        );
        assert!(matches!(result, Err(RunPluginError::PreopenDir { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_dir_is_rejected() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        use std::os::unix::fs::symlink;
        let tmp = camino_tempfile::tempdir().expect("create tempdir");
        let base = tmp.path();
        std::fs::create_dir_all(base.join("real")).expect("create real dir");
        symlink(base.join("real"), base.join("link")).expect("create symlink");

        let result = run_plugin(
            wasm_path.into(),
            base.to_path_buf(),
            vec!["link".to_string()],
            vec![],
            anon(),
            dummy_agent(),
            None,
            anon(),
            "test".to_string(),
            None,
        );
        assert!(matches!(result, Err(RunPluginError::SymlinkDir { .. })));
    }

    #[test]
    fn plugin_success_returns_ok() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let result = run_plugin(
            wasm_path.into(),
            ".".into(),
            vec![],
            vec![],
            anon(),
            dummy_agent(),
            None,
            anon(),
            "ok".to_string(),
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn plugin_failure_maps_to_run_plugin_error() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let result = run_plugin(
            wasm_path.into(),
            ".".into(),
            vec![],
            vec![],
            anon(),
            dummy_agent(),
            None,
            anon(),
            "error".to_string(),
            None,
        );
        assert!(matches!(
            result,
            Err(RunPluginError::PluginFailed { ref message }) if message == "deliberate failure"
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn plugin_stdout_forwarded_through_stdio_channel() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(16);
        let result = tokio::task::block_in_place(|| {
            run_plugin(
                wasm_path.into(),
                ".".into(),
                vec![],
                vec![],
                anon(),
                dummy_agent(),
                None,
                anon(),
                "print".to_string(),
                Some(tx),
            )
        });
        assert!(result.is_ok());
        let msg = rx.try_recv().expect("expected stdout message on channel");
        assert!(msg.contains("stdout from plugin"), "got: {msg}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn plugin_stderr_lines_returned_as_persistent_output() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(16);
        let result = tokio::task::block_in_place(|| {
            run_plugin(
                wasm_path.into(),
                ".".into(),
                vec![],
                vec![],
                anon(),
                dummy_agent(),
                None,
                anon(),
                "hello".to_string(),
                Some(tx),
            )
        });
        let lines = result.expect("plugin should succeed");
        assert_eq!(lines, vec!["hello".to_string()]);
        // The same line is forwarded to the rolling-view channel.
        let live = rx.try_recv().expect("expected stderr line on channel");
        assert!(live.contains("hello"), "got: {live}");
    }
}
