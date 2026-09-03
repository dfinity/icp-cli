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
/// Default seconds of pure wasm compute a plugin may use (host-call latency is
/// excluded). This is a runaway guard, not a security boundary: the plugin runs
/// locally in a read-only WASI sandbox, so the limit only protects the machine
/// running `icp sync` from a plugin that never terminates. Legitimately heavy
/// plugins (e.g. brotli-compressing a large asset bundle) can exceed it,
/// especially on slower CI runners, so it is overridable via the
/// [`PLUGIN_COMPUTE_LIMIT_ENV`] environment variable.
pub const DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS: u64 = 60;
/// Environment variable that overrides [`DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS`].
pub const PLUGIN_COMPUTE_LIMIT_ENV: &str = "ICP_CLI_PLUGIN_COMPUTE_LIMIT_SECS";

use bytes::Bytes;
use camino::Utf8PathBuf;
use candid::{Encode, Principal};
use ic_agent::Agent;
use semver::{Version, VersionReq};
use snafu::prelude::*;
// Aliased because wasmtime-wasi also has an `OutputStream` (imported below).
use icp_events::{OutputStream as EventStream, StepReporter};
use tokio::io::{self, AsyncWrite};
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::cli::{IsTerminal, StdoutStream};
use wasmtime_wasi::p2::{OutputStream, Pollable, StreamError};
use wasmtime_wasi::{DirPerms, FilePerms};

// Both the current and the legacy plugin interfaces are bound, each in its own
// module so their generated type names don't collide. `run_plugin` reads the
// interface version from the component's own metadata (see `detect_plugin_abi`)
// and drives it through the matching module, so plugins built against either
// interface load. The two interfaces are currently structurally identical; the
// split exists so later breaking changes to the current interface can land
// without dropping support for already-built plugins.
mod v2 {
    wasmtime::component::bindgen!({
        world: "sync-plugin",
        path: "sync-plugin.wit",
    });
}

mod v1 {
    wasmtime::component::bindgen!({
        world: "sync-plugin",
        path: "sync-plugin-v1.wit",
    });
}

use v2::icp::sync_plugin::types::CallType;

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

impl HostState {
    /// Perform a canister call to the canister being synced. Shared by both
    /// interface versions.
    fn do_canister_call(
        &mut self,
        method: String,
        arg_bytes: Vec<u8>,
        call_type: CallType,
        direct: bool,
        cycles: u64,
    ) -> Result<Vec<u8>, String> {
        use icp_canister_interfaces::proxy::{ProxyArgs, ProxyResult};

        let cid = self.target_canister_id;
        let agent = Arc::clone(&self.agent);
        let proxy = if direct { None } else { self.proxy };

        // We are already inside tokio::task::block_in_place (see sync/plugin.rs),
        // so blocking the thread here is safe.
        let start = Instant::now();
        let result = tokio::runtime::Handle::current().block_on(async move {
            match call_type {
                CallType::Update => {
                    if let Some(proxy_cid) = proxy {
                        let proxy_args = ProxyArgs {
                            canister_id: cid,
                            method: method.clone(),
                            args: arg_bytes,
                            cycles: candid::Nat::from(cycles),
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

// -- v0.2.0 interface. ---------------------------------------------------------

// `types::Host` is an empty marker trait generated for the `types` interface.
impl v2::icp::sync_plugin::types::Host for HostState {}

impl v2::SyncPluginImports for HostState {
    fn canister_call(
        &mut self,
        req: v2::icp::sync_plugin::types::CanisterCallRequest,
    ) -> Result<Vec<u8>, String> {
        self.do_canister_call(req.method, req.arg, req.call_type, req.direct, req.cycles)
    }
}

// -- v0.1.0 interface. ---------------------------------------------------------

impl v1::icp::sync_plugin::types::Host for HostState {}

impl v1::SyncPluginImports for HostState {
    fn canister_call(
        &mut self,
        req: v1::icp::sync_plugin::types::CanisterCallRequest,
    ) -> Result<Vec<u8>, String> {
        // v1's `call-type` is a distinct generated enum; map it to the shared one.
        let call_type = match req.call_type {
            v1::icp::sync_plugin::types::CallType::Update => CallType::Update,
            v1::icp::sync_plugin::types::CallType::Query => CallType::Query,
        };
        self.do_canister_call(req.method, req.arg, call_type, req.direct, req.cycles)
    }
}

// Used as the error payload inside the epoch_deadline_callback closure, which
// must return wasmtime::Error (= anyhow::Error). Snafu derives std::error::Error
// so .into() converts it via anyhow's blanket From<impl StdError + Send + Sync>.
#[derive(Debug, Snafu)]
#[snafu(display(
    "plugin exceeded the {limit_secs}s compute-time limit. If this plugin legitimately needs more compute time (e.g. brotli-compressing a large asset bundle), raise the limit by setting {PLUGIN_COMPUTE_LIMIT_ENV} above {limit_secs}s."
))]
struct ComputeTimeLimitExceeded {
    limit_secs: u64,
}

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

    #[snafu(display(
        "plugin file '{name}' is not a safe relative path (no absolute paths or '..' allowed)"
    ))]
    UnsafeFile { name: String },

    #[snafu(display(
        "plugin file '{name}' resolves through a symlink ('{link}'); symlinks are not allowed in plugin files"
    ))]
    SymlinkFile { name: String, link: Utf8PathBuf },

    #[snafu(display("failed to read plugin input file at {path}"))]
    ReadFile {
        source: std::io::Error,
        path: Utf8PathBuf,
    },

    #[snafu(display("failed to instantiate wasm component at {path}"))]
    Instantiate {
        source: wasmtime::Error,
        path: Utf8PathBuf,
    },

    #[snafu(display(
        "wasm component at {path} does not implement a supported sync-plugin interface ({detail}). \
         Supported: icp:sync-plugin@0.1 and icp:sync-plugin@0.2."
    ))]
    UnsupportedInterface { path: Utf8PathBuf, detail: String },

    #[snafu(display("failed to call exec() on plugin at {path}"))]
    CallExec {
        source: wasmtime::Error,
        path: Utf8PathBuf,
    },

    #[snafu(display("plugin returned error: {message}"))]
    PluginFailed { message: String },
}

/// Which version of the sync-plugin interface a component was built against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PluginAbi {
    /// Current interface (`icp:sync-plugin@0.2.x`).
    V2,
    /// Legacy interface (`icp:sync-plugin@0.1.x`).
    V1,
}

/// The interface package a `use`-ing plugin component imports, whose version we
/// read to pick the ABI. wit-bindgen emits this import for any world that pulls
/// types from the interface, so it is present on every real plugin.
const TYPES_INTERFACE_PREFIX: &str = "icp:sync-plugin/types@";

/// Determine which interface a component implements by reading the version off
/// its imported `icp:sync-plugin/types@<version>` instance — the plugin's own
/// declared metadata — rather than probing with a trial instantiation. The
/// version is matched with semver caret requirements, so each supported minor
/// (the breaking unit for 0.x) accepts any patch release within it.
fn detect_plugin_abi(
    engine: &Engine,
    component: &Component,
    wasm_path: &Utf8PathBuf,
) -> Result<PluginAbi, RunPluginError> {
    let raw = component
        .component_type()
        .imports(engine)
        .find_map(|(name, _)| name.strip_prefix(TYPES_INTERFACE_PREFIX).map(str::to_owned));

    let Some(raw) = raw else {
        return UnsupportedInterfaceSnafu {
            path: wasm_path.clone(),
            detail: format!("no {TYPES_INTERFACE_PREFIX}<version> import found"),
        }
        .fail();
    };

    let version = Version::parse(&raw).map_err(|source| {
        UnsupportedInterfaceSnafu {
            path: wasm_path.clone(),
            detail: format!("interface version '{raw}' is not valid semver: {source}"),
        }
        .build()
    })?;

    // `^0.1`/`^0.2` follow semver's 0.x rule: they match within the minor and
    // exclude the next one (>=0.1.0, <0.2.0 and >=0.2.0, <0.3.0 respectively).
    if VersionReq::parse("^0.2")
        .expect("valid req")
        .matches(&version)
    {
        Ok(PluginAbi::V2)
    } else if VersionReq::parse("^0.1")
        .expect("valid req")
        .matches(&version)
    {
        Ok(PluginAbi::V1)
    } else {
        UnsupportedInterfaceSnafu {
            path: wasm_path.clone(),
            detail: format!("unsupported interface version {version}"),
        }
        .fail()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_plugin(
    wasm_path: Utf8PathBuf,
    base_dir: Utf8PathBuf,
    dirs: Vec<String>,
    files: Vec<String>,
    target_canister_id: Principal,
    agent: Agent,
    proxy: Option<Principal>,
    identity_principal: Principal,
    environment: String,
    compute_limit_secs: u64,
    reporter: StepReporter,
) -> Result<Vec<String>, RunPluginError> {
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
        ensure!(!crate::path::escapes_base(dir), UnsafeDirSnafu { dir });
        // Reject symlinks in the declared path: neither the final entry nor any
        // intermediate component may be a symlink, so the preopen cannot escape
        // `base_dir` to a target elsewhere on disk. (Symlinks *inside* a preopen
        // that escape it are separately rejected by the WASI sandbox.)
        if let Some(link) = crate::path::first_symlink_component(&base_dir, dir) {
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

    // Read each declared file on the host and pass its content inline. The same
    // path-safety checks as `dirs` apply: reject escaping or symlinked paths so
    // a read cannot leave `base_dir`.
    // Held as plain (name, content) pairs so they can be converted to whichever
    // interface version's `file-input` record the plugin turns out to use.
    let mut file_contents: Vec<(String, String)> = Vec::with_capacity(files.len());
    for name in &files {
        ensure!(!crate::path::escapes_base(name), UnsafeFileSnafu { name });
        if let Some(link) = crate::path::first_symlink_component(&base_dir, name) {
            return SymlinkFileSnafu { name, link }.fail();
        }
        let path = base_dir.join(name);
        let content =
            std::fs::read_to_string(path.as_std_path()).context(ReadFileSnafu { path })?;
        file_contents.push((name.clone(), content));
    }

    let persistent_stderr: Arc<StdMutex<Vec<String>>> = Arc::default();
    let stdout_capture = LineCapture::new("stdout", EventStream::Stdout, reporter.clone(), None);
    let stderr_capture = LineCapture::new(
        "stderr",
        EventStream::Stderr,
        reporter.clone(),
        Some(persistent_stderr.clone()),
    );
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

    let mut store = Store::new(&engine, host_state);
    store.set_epoch_deadline(compute_limit_secs);
    store.epoch_deadline_callback(move |_| {
        let extra = epoch_extension.swap(0, Ordering::Relaxed);
        if extra > 0 {
            Ok(wasmtime::UpdateDeadline::Continue(extra))
        } else {
            Err(ComputeTimeLimitExceeded {
                limit_secs: compute_limit_secs,
            }
            .into())
        }
    });

    let canister_id_text = target_canister_id.to_text();
    let identity_text = identity_principal.to_text();
    let proxy_text = proxy.map(|p| p.to_text());

    // Which interface the plugin was built against is read from the component's
    // own declared metadata (see `detect_plugin_abi`) rather than probed by
    // trial instantiation, then driven through the matching bindgen world.
    let call_result = match detect_plugin_abi(&engine, &component, &wasm_path)? {
        PluginAbi::V2 => {
            let mut linker: Linker<HostState> = Linker::new(&engine);
            wasmtime_wasi::p2::add_to_linker_sync(&mut linker).context(InstantiateSnafu {
                path: wasm_path.clone(),
            })?;
            v2::SyncPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s).context(
                InstantiateSnafu {
                    path: wasm_path.clone(),
                },
            )?;
            let plugin = v2::SyncPlugin::instantiate(&mut store, &component, &linker).context(
                InstantiateSnafu {
                    path: wasm_path.clone(),
                },
            )?;
            let input = v2::SyncExecInput {
                canister_id: canister_id_text,
                environment,
                dirs,
                files: file_contents
                    .into_iter()
                    .map(|(name, content)| v2::FileInput { name, content })
                    .collect(),
                identity_principal: identity_text,
                proxy_canister_id: proxy_text,
            };
            plugin.call_exec(&mut store, &input)
        }
        PluginAbi::V1 => {
            let mut linker: Linker<HostState> = Linker::new(&engine);
            wasmtime_wasi::p2::add_to_linker_sync(&mut linker).context(InstantiateSnafu {
                path: wasm_path.clone(),
            })?;
            v1::SyncPlugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s).context(
                InstantiateSnafu {
                    path: wasm_path.clone(),
                },
            )?;
            let plugin = v1::SyncPlugin::instantiate(&mut store, &component, &linker).context(
                InstantiateSnafu {
                    path: wasm_path.clone(),
                },
            )?;
            let input = v1::SyncExecInput {
                canister_id: canister_id_text,
                environment,
                dirs,
                files: file_contents
                    .into_iter()
                    .map(|(name, content)| v1::FileInput { name, content })
                    .collect(),
                identity_principal: identity_text,
                proxy_canister_id: proxy_text,
            };
            plugin.call_exec(&mut store, &input)
        }
    };

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
// newlines; complete lines have ANSI escapes stripped and are emitted as
// output events on the step reporter (non-blocking). For stderr, the same
// lines are also appended to `persistent`, which is drained by
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
    stream: EventStream,
    reporter: StepReporter,
    persistent: Option<Arc<StdMutex<Vec<String>>>>,
}

impl LineCapture {
    fn new(
        label: &'static str,
        stream: EventStream,
        reporter: StepReporter,
        persistent: Option<Arc<StdMutex<Vec<String>>>>,
    ) -> Self {
        Self {
            state: Arc::default(),
            label,
            stream,
            reporter,
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
        self.reporter.output(self.stream, line.clone());
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
            DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS,
            StepReporter::null(),
        );
        assert!(matches!(result, Err(RunPluginError::LoadComponent { .. })));
    }

    #[test]
    fn compute_time_limit_error_reflects_the_configured_limit() {
        // The remediation must anchor to the actual limit (not a hardcoded
        // literal), so it reads correctly whether the limit is the default or
        // an env-var override. Use a distinctive value to catch a regression.
        let msg = ComputeTimeLimitExceeded { limit_secs: 120 }.to_string();
        assert!(msg.contains("exceeded the 120s"), "got: {msg}");
        // The suggestion tells the user to go above the current limit — the
        // value must flow into the remediation clause too.
        assert!(msg.contains("above 120s"), "got: {msg}");
        assert!(msg.contains(PLUGIN_COMPUTE_LIMIT_ENV), "got: {msg}");
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
            DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS,
            StepReporter::null(),
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
            DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS,
            StepReporter::null(),
        );
        assert!(matches!(result, Err(RunPluginError::SymlinkDir { .. })));
    }

    #[test]
    fn read_file_error_on_missing_file() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let result = run_plugin(
            wasm_path.into(),
            ".".into(),
            vec![],
            vec!["nonexistent_file.txt".to_string()],
            anon(),
            dummy_agent(),
            None,
            anon(),
            "test".to_string(),
            DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS,
            StepReporter::null(),
        );
        assert!(matches!(result, Err(RunPluginError::ReadFile { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_file_is_rejected() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        use std::os::unix::fs::symlink;
        let tmp = camino_tempfile::tempdir().expect("create tempdir");
        let base = tmp.path();
        std::fs::write(base.join("real.txt"), b"data").expect("write real file");
        symlink(base.join("real.txt"), base.join("link.txt")).expect("create symlink");

        let result = run_plugin(
            wasm_path.into(),
            base.to_path_buf(),
            vec![],
            vec!["link.txt".to_string()],
            anon(),
            dummy_agent(),
            None,
            anon(),
            "test".to_string(),
            DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS,
            StepReporter::null(),
        );
        assert!(matches!(result, Err(RunPluginError::SymlinkFile { .. })));
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
            DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS,
            StepReporter::null(),
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
            DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS,
            StepReporter::null(),
        );
        assert!(matches!(
            result,
            Err(RunPluginError::PluginFailed { ref message }) if message == "deliberate failure"
        ));
    }

    #[test]
    fn plugin_exceeding_compute_limit_is_trapped() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        // The "spin" fixture busy-loops forever; a 1-second limit keeps the
        // test fast while still exercising the epoch-interruption trap.
        let result = run_plugin(
            wasm_path.into(),
            ".".into(),
            vec![],
            vec![],
            anon(),
            dummy_agent(),
            None,
            anon(),
            "spin".to_string(),
            1,
            StepReporter::null(),
        );
        let err = result.expect_err("spinning plugin should hit the compute limit");
        // The trap surfaces through the CallExec source chain, so walk it and
        // assert the message names both the limit and the override env var.
        let mut chain = err.to_string();
        let mut cur: &dyn std::error::Error = &err;
        while let Some(src) = cur.source() {
            chain = format!("{chain}: {src}");
            cur = src;
        }
        assert!(
            chain.contains("compute-time limit") && chain.contains(PLUGIN_COMPUTE_LIMIT_ENV),
            "unexpected error chain: {chain}"
        );
    }

    /// Drain the emitted output events into (stream, line) pairs.
    fn output_lines(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<icp_events::Event<()>>,
    ) -> Vec<(EventStream, String)> {
        let mut lines = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let icp_events::EventKind::Output { stream, line } = event.kind {
                lines.push((stream, line));
            }
        }
        lines
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn plugin_stdout_forwarded_through_step_reporter() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        // The task payload type is irrelevant here: only step output is observed.
        let (reporter, mut rx) = icp_events::step_channel::<()>();
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
                DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS,
                reporter,
            )
        });
        assert!(result.is_ok());
        let lines = output_lines(&mut rx);
        assert!(
            lines
                .iter()
                .any(|(stream, line)| matches!(stream, EventStream::Stdout)
                    && line.contains("stdout from plugin")),
            "got: {lines:?}"
        );
    }

    #[test]
    fn legacy_v1_plugin_is_detected_and_driven() {
        // A plugin built against the v0.1.0 interface must still load: the host
        // reads its declared interface version and drives it through the v1 path.
        let Some(wasm_path) = option_env!("TEST_PLUGIN_V1_WASM") else {
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
            DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS,
            StepReporter::null(),
        );
        assert!(result.is_ok());
        // Its error surface flows through the same machinery as v0.2.0 plugins.
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
            DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS,
            StepReporter::null(),
        );
        assert!(matches!(
            result,
            Err(RunPluginError::PluginFailed { ref message }) if message == "deliberate v1 failure"
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn plugin_stderr_lines_returned_as_persistent_output() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        // The task payload type is irrelevant here: only step output is observed.
        let (reporter, mut rx) = icp_events::step_channel::<()>();
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
                DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS,
                reporter,
            )
        });
        let lines = result.expect("plugin should succeed");
        assert_eq!(lines, vec!["hello".to_string()]);
        // The same line is also streamed as a live stderr event.
        let live = output_lines(&mut rx);
        assert!(
            live.iter()
                .any(|(stream, line)| matches!(stream, EventStream::Stderr)
                    && line.contains("hello")),
            "got: {live:?}"
        );
    }
}
