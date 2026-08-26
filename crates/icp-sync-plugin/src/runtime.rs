// Host-side Component Model runtime for sync plugins.
use std::collections::BTreeMap;
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
// interface load.
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

use v2::icp::sync_plugin::types::{CallTarget, CallType, CanisterIdEntry};

/// A manifest path passed to a plugin, tagged with the map key it was declared
/// under. Both `dirs` and `files` are lists of these.
///
/// The key is `None` when the manifest wrote the setting as a plain list, and
/// `Some(name)` when it wrote a map. It is *non-unique*: several paths share a
/// key when a map key resolves to a list of paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyedPath {
    /// The map key this path was declared under, or `None` for a plain-list entry.
    pub key: Option<String>,
    /// Manifest-relative path, anchored at the invocation's `base_dir`.
    pub path: String,
}

/// A declared file the host read: the key and path it was declared under, plus
/// its content. Held version-agnostically so it can be converted to whichever
/// interface version's `file-input` record the plugin turns out to use — the
/// v0.1.0 record has no `key`, so it is dropped there.
struct FileContent {
    key: Option<String>,
    name: String,
    content: String,
}

/// The canisters a sync plugin is permitted to call, beyond the canister being
/// synced (which is always reachable via [`CallTarget::Host`]).
///
/// Built by the CLI from the plugin step's `canisters` list, resolved against
/// the project's canister ID table. Keeping the resolution on the CLI side
/// keeps this runtime crate free of any manifest knowledge.
#[derive(Clone, Debug, Default)]
pub struct CallableCanisters {
    /// Canisters callable by name ([`CallTarget::Name`]). Maps the name — as it
    /// appears in the canister ID table — to the principal it resolves to.
    pub by_name: BTreeMap<String, Principal>,
}

/// Resolve a plugin-supplied [`CallTarget`] to a concrete principal, enforcing
/// that the plugin listed it in `canisters`. The canister being synced (`host`)
/// is always permitted.
fn resolve_call_target(
    target: &CallTarget,
    host_canister_id: Principal,
    callable: &CallableCanisters,
) -> Result<Principal, String> {
    match target {
        CallTarget::Host => Ok(host_canister_id),
        CallTarget::Name(name) => callable.by_name.get(name).copied().ok_or_else(|| {
            format!(
                "plugin is not permitted to call canister '{name}': declare it in the sync step's \
                 `canisters` list to allow it"
            )
        }),
    }
}

// HostState holds everything the plugin's import functions need.
struct HostState {
    /// The canister being synced — the target of [`CallTarget::Host`] calls.
    host_canister_id: Principal,
    /// Canisters the plugin declared in `canisters` and may also call.
    callable: CallableCanisters,
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
    /// Perform a canister call to an already-resolved target principal. Shared
    /// by both interface versions: the v0.1.0 import always passes the canister
    /// being synced; the v0.2.0 import passes the resolved `call-target`.
    fn do_canister_call(
        &mut self,
        target: Principal,
        method: String,
        arg_bytes: Vec<u8>,
        call_type: CallType,
        direct: bool,
        cycles: u64,
    ) -> Result<Vec<u8>, String> {
        use icp_canister_interfaces::proxy::{ProxyArgs, ProxyResult};

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
                            canister_id: target,
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
                            .update(&target, &method)
                            .with_arg(arg_bytes)
                            .await
                            .map_err(|e| format!("canister call failed: {e}"))
                    }
                }
                CallType::Query => agent
                    .query(&target, &method)
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

// -- v0.2.0 interface: the plugin chooses the target via `call-target`. --------

// `types::Host` is an empty marker trait generated for the `types` interface.
impl v2::icp::sync_plugin::types::Host for HostState {}

impl v2::SyncPluginImports for HostState {
    fn canister_call(
        &mut self,
        req: v2::icp::sync_plugin::types::CanisterCallRequest,
    ) -> Result<Vec<u8>, String> {
        let target = resolve_call_target(&req.target, self.host_canister_id, &self.callable)?;
        self.do_canister_call(
            target,
            req.method,
            req.arg,
            req.call_type,
            req.direct,
            req.cycles,
        )
    }
}

// -- v0.1.0 interface: calls always go to the canister being synced. -----------

impl v1::icp::sync_plugin::types::Host for HostState {}

impl v1::SyncPluginImports for HostState {
    fn canister_call(
        &mut self,
        req: v1::icp::sync_plugin::types::CanisterCallRequest,
    ) -> Result<Vec<u8>, String> {
        // The legacy interface has no target field; always call the host canister.
        let target = self.host_canister_id;
        // v1's `call-type` is a distinct generated enum; map it to the shared one.
        let call_type = match req.call_type {
            v1::icp::sync_plugin::types::CallType::Update => CallType::Update,
            v1::icp::sync_plugin::types::CallType::Query => CallType::Query,
        };
        self.do_canister_call(
            target, req.method, req.arg, call_type, req.direct, req.cycles,
        )
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

    #[snafu(display("plugin dir '{dir}' is not a relative path (absolute paths are not allowed)"))]
    UnsafeDir { dir: String },

    #[snafu(display(
        "plugin dir '{dir}' resolves outside '{project_dir}'; \
         a plugin may only read paths inside the project directory"
    ))]
    DirOutsideProject {
        dir: String,
        project_dir: Utf8PathBuf,
    },

    #[snafu(display(
        "plugin dir '{dir}' resolves through a symlink ('{link}'); symlinks are not allowed in plugin dirs"
    ))]
    SymlinkDir { dir: String, link: Utf8PathBuf },

    #[snafu(display("plugin dir '{dir}' is not an existing directory"))]
    MissingDir { dir: String },

    #[snafu(display("failed to preopen directory '{dir}' for the plugin"))]
    PreopenDir {
        source: wasmtime::Error,
        dir: Utf8PathBuf,
    },

    #[snafu(display(
        "plugin file '{name}' is not a relative path (absolute paths are not allowed)"
    ))]
    UnsafeFile { name: String },

    #[snafu(display(
        "plugin file '{name}' resolves outside '{project_dir}'; \
         a plugin may only read paths inside the project directory"
    ))]
    FileOutsideProject {
        name: String,
        project_dir: Utf8PathBuf,
    },

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
    /// Current interface (`icp:sync-plugin@0.2.x`): `canister-call` chooses a
    /// target and `sync-exec-input` carries the canister ID table.
    V2,
    /// Legacy interface (`icp:sync-plugin@0.1.x`): calls always reach the
    /// canister being synced.
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

/// Everything [`run_plugin`] needs to load and drive one sync plugin.
#[derive(Debug)]
pub struct PluginInvocation {
    /// On-disk path to the plugin's wasm component.
    pub wasm_path: Utf8PathBuf,
    /// Directory the declared `dirs`/`files` are anchored at (the canister dir).
    pub base_dir: Utf8PathBuf,
    /// The project directory: the sandbox boundary. A declared path may rise
    /// out of `base_dir` with `..` and reach anything inside the project, but
    /// nothing above it.
    ///
    /// A `base_dir` that does not lie within this directory — a dependency
    /// project reached by an out-of-tree `path:` — is its own boundary instead,
    /// which grants nothing above the canister directory.
    pub project_dir: Utf8PathBuf,
    /// Manifest-relative directories to preopen read-only, each tagged with the
    /// map key it was declared under (if any).
    pub dirs: Vec<KeyedPath>,
    /// Manifest-relative files to read and pass inline, each tagged with the map
    /// key it was declared under (if any).
    pub files: Vec<KeyedPath>,
    /// Key-value fields to pass inline. Passed to v0.2.0 plugins; ignored by
    /// v0.1.0 plugins, whose interface has no `fields`.
    pub fields: BTreeMap<String, String>,
    /// The canister being synced. Reachable via `call-target::host`.
    pub host_canister_id: Principal,
    /// Agent used for canister calls.
    pub agent: Agent,
    /// Proxy canister to route update calls through, if configured.
    pub proxy: Option<Principal>,
    /// Signing identity principal, surfaced to the plugin.
    pub identity_principal: Principal,
    /// Name of the environment being synced.
    pub environment: String,
    /// Pure-wasm compute-time budget in seconds.
    pub compute_limit_secs: u64,
    /// The project's canister ID table for this environment, as exposed to the
    /// plugin. Same-project canisters appear both under their fully-qualified
    /// key and their bare local name (see the WIT `canister-id-entry` docs).
    pub canister_ids: BTreeMap<String, Principal>,
    /// Canisters the plugin declared in `canisters` and may call, beyond the
    /// canister being synced. Ignored by v0.1.0 plugins, which can only reach
    /// the canister being synced.
    pub callable: CallableCanisters,
    /// Reporter the plugin's live stdout/stderr is emitted on.
    pub reporter: StepReporter,
}

pub fn run_plugin(invocation: PluginInvocation) -> Result<Vec<String>, RunPluginError> {
    let PluginInvocation {
        wasm_path,
        base_dir,
        project_dir,
        dirs,
        files,
        fields,
        host_canister_id,
        agent,
        proxy,
        identity_principal,
        environment,
        compute_limit_secs,
        canister_ids,
        callable,
        reporter,
    } = invocation;

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

    // Declared paths are written relative to the canister directory but are
    // resolved against — and confined to — the project directory, so an entry
    // may reach a sibling canister's tree with `..` while nothing outside the
    // project is reachable. A canister directory that lies outside the project
    // is its own root, which grants nothing above it (see `project_dir`).
    let (root, base_rel) = match crate::path::base_within_root(&project_dir, &base_dir) {
        Some(base_rel) => (&project_dir, base_rel),
        None => (&base_dir, Vec::new()),
    };

    // Check every declared directory: each one is handed to the plugin as
    // configuration, so it is rejected for being unsafe or unusable whether or
    // not it ends up needing a preopen of its own. The resolved host paths are
    // kept for the preopens below, keyed by the spelling they were declared as.
    let mut host_paths: BTreeMap<&str, Utf8PathBuf> = BTreeMap::new();
    for KeyedPath { path: dir, .. } in &dirs {
        let resolved = match crate::path::resolve(&base_rel, dir) {
            Ok(resolved) => resolved,
            Err(crate::path::Escape::NotRelative) => return UnsafeDirSnafu { dir }.fail(),
            Err(crate::path::Escape::AboveRoot) => {
                return DirOutsideProjectSnafu {
                    dir,
                    project_dir: root,
                }
                .fail();
            }
        };
        // Reject symlinks in the resolved path: neither the final entry nor any
        // intermediate component may be a symlink, so the preopen cannot escape
        // the project to a target elsewhere on disk. (Symlinks *inside* a
        // preopen that escape it are separately rejected by the WASI sandbox.)
        if let Some(link) = resolved.first_symlink_component(root) {
            return SymlinkDirSnafu { dir, link }.fail();
        }
        let host_path = root.join(resolved.path());
        let is_dir = std::fs::metadata(host_path.as_std_path()).is_ok_and(|meta| meta.is_dir());
        ensure!(is_dir, MissingDirSnafu { dir });
        host_paths.insert(dir, host_path);
    }

    // Preopen read-only, one per distinct tree — a directory declared twice, or
    // one already reachable through a declared ancestor, needs no preopen of its
    // own. The guest sees each preopen at the same relative path it used in the
    // manifest, and reaches a nested declared directory through its ancestor.
    let mut wasi_builder = wasmtime_wasi::WasiCtxBuilder::new();
    for dir in crate::path::covering_dirs(dirs.iter().map(|d| d.path.as_str())) {
        // `covering_dirs` returns a subset of the declared spellings, every one
        // of which the loop above resolved.
        let host_path = host_paths
            .get(dir)
            .expect("covering dir was not among the declared dirs");
        wasi_builder
            .preopened_dir(
                host_path.as_std_path(),
                dir,
                DirPerms::READ,
                FilePerms::READ,
            )
            .context(PreopenDirSnafu {
                dir: host_path.clone(),
            })?;
    }

    // Read each declared file on the host and pass its content inline. The same
    // path-safety checks as `dirs` apply: reject unsafe, escaping, or symlinked
    // paths so a read cannot leave the project.
    let mut file_contents: Vec<FileContent> = Vec::with_capacity(files.len());
    for KeyedPath { key, path: name } in &files {
        let resolved = match crate::path::resolve(&base_rel, name) {
            Ok(resolved) => resolved,
            Err(crate::path::Escape::NotRelative) => return UnsafeFileSnafu { name }.fail(),
            Err(crate::path::Escape::AboveRoot) => {
                return FileOutsideProjectSnafu {
                    name,
                    project_dir: root,
                }
                .fail();
            }
        };
        if let Some(link) = resolved.first_symlink_component(root) {
            return SymlinkFileSnafu { name, link }.fail();
        }
        let path = root.join(resolved.path());
        let content =
            std::fs::read_to_string(path.as_std_path()).context(ReadFileSnafu { path })?;
        file_contents.push(FileContent {
            key: key.clone(),
            name: name.clone(),
            content,
        });
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
        host_canister_id,
        callable,
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

    let canister_id_text = host_canister_id.to_text();
    let identity_text = identity_principal.to_text();
    let proxy_text = proxy.map(|p| p.to_text());

    // Which interface the plugin was built against is read from the component's
    // own declared metadata (see `detect_plugin_abi`) rather than probed by
    // trial instantiation. Both are served in parallel: v0.2.0 plugins choose a
    // call target and receive the canister ID table; v0.1.0 plugins get neither
    // and always call the canister being synced.
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
                dirs: dirs
                    .into_iter()
                    .map(|KeyedPath { key, path }| v2::DirInput { key, path })
                    .collect(),
                files: file_contents
                    .into_iter()
                    .map(|FileContent { key, name, content }| v2::FileInput { key, name, content })
                    .collect(),
                fields: fields
                    .into_iter()
                    .map(|(name, value)| v2::FieldInput { name, value })
                    .collect(),
                identity_principal: identity_text,
                proxy_canister_id: proxy_text,
                canister_ids: canister_ids
                    .into_iter()
                    .map(|(name, id)| CanisterIdEntry {
                        name,
                        id: id.to_text(),
                    })
                    .collect(),
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
                // The v0.1.0 interface has no per-entry key; pass just the paths.
                dirs: dirs
                    .into_iter()
                    .map(|KeyedPath { path, .. }| path)
                    .collect(),
                // The v0.1.0 `file-input` record has no `key`; drop it.
                files: file_contents
                    .into_iter()
                    .map(|FileContent { name, content, .. }| v1::FileInput { name, content })
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

    /// Plain (unkeyed) [`KeyedPath`]s, as a plain-list manifest entry produces.
    fn unkeyed(paths: &[&str]) -> Vec<KeyedPath> {
        paths
            .iter()
            .map(|p| KeyedPath {
                key: None,
                path: (*p).to_string(),
            })
            .collect()
    }

    /// A [`PluginInvocation`] with test-friendly defaults: anonymous canister
    /// and identity, no proxy, no declared callable canisters, the default
    /// compute limit, and the current directory as both the base and the
    /// project. Tests override the few fields they care about.
    fn invocation(wasm_path: &str, environment: &str) -> PluginInvocation {
        PluginInvocation {
            wasm_path: wasm_path.into(),
            base_dir: ".".into(),
            project_dir: ".".into(),
            dirs: vec![],
            files: vec![],
            fields: BTreeMap::new(),
            host_canister_id: anon(),
            agent: dummy_agent(),
            proxy: None,
            identity_principal: anon(),
            environment: environment.to_string(),
            compute_limit_secs: DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS,
            canister_ids: BTreeMap::new(),
            callable: CallableCanisters::default(),
            reporter: StepReporter::null(),
        }
    }

    // -------------------------------------------------------------------------
    // Call-target resolution (enforcement) — pure logic, no fixture WASM needed
    // -------------------------------------------------------------------------

    #[test]
    fn resolve_target_host_is_always_allowed() {
        let host = Principal::from_slice(&[1; 4]);
        let callable = CallableCanisters::default();
        assert_eq!(
            resolve_call_target(&CallTarget::Host, host, &callable).unwrap(),
            host
        );
    }

    #[test]
    fn resolve_target_name_requires_declaration() {
        let host = Principal::from_slice(&[1; 4]);
        let dep = Principal::from_slice(&[2; 4]);
        let callable = CallableCanisters {
            by_name: BTreeMap::from([("backend".to_string(), dep)]),
        };
        assert_eq!(
            resolve_call_target(&CallTarget::Name("backend".into()), host, &callable).unwrap(),
            dep
        );
        let err = resolve_call_target(&CallTarget::Name("frontend".into()), host, &callable)
            .expect_err("undeclared name must be rejected");
        assert!(
            err.contains("not permitted") && err.contains("frontend"),
            "got: {err}"
        );
    }

    // -------------------------------------------------------------------------
    // Error-path tests — no fixture WASM needed
    // -------------------------------------------------------------------------

    #[test]
    fn load_component_error_on_missing_file() {
        let result = run_plugin(invocation("nonexistent.wasm", "test"));
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
    fn missing_dir_is_rejected() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let mut inv = invocation(wasm_path, "test");
        inv.dirs = unkeyed(&["nonexistent_dir"]);
        assert!(matches!(
            run_plugin(inv),
            Err(RunPluginError::MissingDir { .. })
        ));
    }

    /// A directory declared under several keys, or nested inside another
    /// declared one, reaches the plugin as every entry it was written as. Only
    /// the preopens behind those entries collapse — `data/inner` has none of its
    /// own here, and is read through the `data` preopen that covers it.
    #[test]
    fn aliased_and_nested_dirs_are_all_readable() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let tmp = camino_tempfile::tempdir().expect("create tempdir");
        let base = tmp.path();
        std::fs::create_dir_all(base.join("data/inner")).expect("create dir");
        std::fs::write(base.join("data/top.txt"), b"top").expect("write file");
        std::fs::write(base.join("data/inner/deep.txt"), b"deep").expect("write file");

        let mut inv = invocation(wasm_path, "read-dirs");
        inv.base_dir = base.to_path_buf();
        inv.dirs = [("seed", "data"), ("backup", "data"), ("sub", "data/inner")]
            .into_iter()
            .map(|(key, path)| KeyedPath {
                key: Some(key.to_owned()),
                path: path.to_owned(),
            })
            .collect();

        let lines = run_plugin(inv).expect("plugin should succeed");
        assert_eq!(
            lines,
            [
                "seed=inner,top.txt".to_string(),
                "backup=inner,top.txt".to_string(),
                "sub=deep.txt".to_string(),
            ],
        );
    }

    /// A `dirs:` entry may rise out of the canister directory into the rest of
    /// the project. The guest sees it at the path it was declared as, so it
    /// reads `../shared` verbatim, and a directory below the entry is reached
    /// through the same preopen.
    #[test]
    fn dirs_above_the_canister_dir_are_readable() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let tmp = camino_tempfile::tempdir().expect("create tempdir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("canisters/backend")).expect("create dir");
        std::fs::create_dir_all(root.join("shared/inner")).expect("create dir");
        std::fs::write(root.join("shared/top.txt"), b"top").expect("write file");
        std::fs::write(root.join("shared/inner/deep.txt"), b"deep").expect("write file");

        let mut inv = invocation(wasm_path, "read-dirs");
        inv.base_dir = root.join("canisters/backend");
        inv.project_dir = root.to_path_buf();
        inv.dirs = [("shared", "../../shared"), ("inner", "../../shared/inner")]
            .into_iter()
            .map(|(key, path)| KeyedPath {
                key: Some(key.to_owned()),
                path: path.to_owned(),
            })
            .collect();

        let lines = run_plugin(inv).expect("plugin should succeed");
        assert_eq!(
            lines,
            [
                "shared=inner,top.txt".to_string(),
                "inner=deep.txt".to_string(),
            ],
        );
    }

    /// Two entries where one reaches further out than the other each need their
    /// own preopen: `..` is the canister's parent and `../../shared` a child of
    /// its grandparent, so neither is readable through the other's.
    #[test]
    fn dirs_reaching_out_by_different_amounts_are_both_readable() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let tmp = camino_tempfile::tempdir().expect("create tempdir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("canisters/backend")).expect("create dir");
        std::fs::create_dir_all(root.join("shared")).expect("create dir");
        std::fs::write(root.join("shared/top.txt"), b"top").expect("write file");

        let mut inv = invocation(wasm_path, "read-dirs");
        inv.base_dir = root.join("canisters/backend");
        inv.project_dir = root.to_path_buf();
        inv.dirs = [("siblings", ".."), ("shared", "../../shared")]
            .into_iter()
            .map(|(key, path)| KeyedPath {
                key: Some(key.to_owned()),
                path: path.to_owned(),
            })
            .collect();

        let lines = run_plugin(inv).expect("plugin should succeed");
        assert_eq!(
            lines,
            ["siblings=backend".to_string(), "shared=top.txt".to_string(),],
        );
    }

    /// The project directory is the boundary: an entry that rises above it is
    /// rejected before the plugin runs.
    #[test]
    fn dir_above_the_project_is_rejected() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let tmp = camino_tempfile::tempdir().expect("create tempdir");
        let root = tmp.path().join("project");
        std::fs::create_dir_all(root.join("backend")).expect("create dir");
        std::fs::create_dir_all(tmp.path().join("outside")).expect("create dir");

        let mut inv = invocation(wasm_path, "read-dirs");
        inv.base_dir = root.join("backend");
        inv.project_dir = root.clone();
        inv.dirs = unkeyed(&["../../outside"]);
        assert!(matches!(
            run_plugin(inv),
            Err(RunPluginError::DirOutsideProject { .. })
        ));
    }

    /// A canister directory outside the project — a dependency reached by an
    /// out-of-tree path — is its own boundary, so nothing above it is reachable.
    #[test]
    fn dir_above_an_out_of_project_canister_dir_is_rejected() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let tmp = camino_tempfile::tempdir().expect("create tempdir");
        std::fs::create_dir_all(tmp.path().join("outside/backend")).expect("create dir");
        std::fs::create_dir_all(tmp.path().join("outside/shared")).expect("create dir");

        let mut inv = invocation(wasm_path, "read-dirs");
        inv.base_dir = tmp.path().join("outside/backend");
        inv.project_dir = tmp.path().join("project");
        inv.dirs = unkeyed(&["../shared"]);
        assert!(matches!(
            run_plugin(inv),
            Err(RunPluginError::DirOutsideProject { .. })
        ));
    }

    /// A `files:` entry may reach the rest of the project too; its content is
    /// read by the host and passed inline.
    #[test]
    fn files_above_the_canister_dir_are_read() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let tmp = camino_tempfile::tempdir().expect("create tempdir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("backend")).expect("create dir");
        std::fs::write(root.join("cfg.txt"), b"data").expect("write file");

        let mut inv = invocation(wasm_path, "keys");
        inv.base_dir = root.join("backend");
        inv.project_dir = root.to_path_buf();
        inv.files = unkeyed(&["../cfg.txt"]);

        let lines = run_plugin(inv).expect("plugin should succeed");
        assert_eq!(lines, ["file -=../cfg.txt".to_string()]);
    }

    #[test]
    fn file_above_the_project_is_rejected() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let tmp = camino_tempfile::tempdir().expect("create tempdir");
        let root = tmp.path().join("project");
        std::fs::create_dir_all(root.join("backend")).expect("create dir");
        std::fs::write(tmp.path().join("secret.txt"), b"secret").expect("write file");

        let mut inv = invocation(wasm_path, "keys");
        inv.base_dir = root.join("backend");
        inv.project_dir = root.clone();
        inv.files = unkeyed(&["../../secret.txt"]);
        assert!(matches!(
            run_plugin(inv),
            Err(RunPluginError::FileOutsideProject { .. })
        ));
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

        let mut inv = invocation(wasm_path, "test");
        inv.base_dir = base.to_path_buf();
        inv.dirs = unkeyed(&["link"]);
        assert!(matches!(
            run_plugin(inv),
            Err(RunPluginError::SymlinkDir { .. })
        ));
    }

    #[test]
    fn read_file_error_on_missing_file() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let mut inv = invocation(wasm_path, "test");
        inv.files = unkeyed(&["nonexistent_file.txt"]);
        assert!(matches!(
            run_plugin(inv),
            Err(RunPluginError::ReadFile { .. })
        ));
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

        let mut inv = invocation(wasm_path, "test");
        inv.base_dir = base.to_path_buf();
        inv.files = unkeyed(&["link.txt"]);
        assert!(matches!(
            run_plugin(inv),
            Err(RunPluginError::SymlinkFile { .. })
        ));
    }

    #[test]
    fn plugin_success_returns_ok() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        assert!(run_plugin(invocation(wasm_path, "ok")).is_ok());
    }

    #[test]
    fn plugin_failure_maps_to_run_plugin_error() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        assert!(matches!(
            run_plugin(invocation(wasm_path, "error")),
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
        let mut inv = invocation(wasm_path, "spin");
        inv.compute_limit_secs = 1;
        let err = run_plugin(inv).expect_err("spinning plugin should hit the compute limit");
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
            let mut inv = invocation(wasm_path, "print");
            inv.reporter = reporter;
            run_plugin(inv)
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
    fn plugin_fields_are_passed_through() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        let mut inv = invocation(wasm_path, "fields");
        inv.fields = BTreeMap::from([
            ("greeting".to_string(), "hi".to_string()),
            ("audience".to_string(), "world".to_string()),
        ]);
        // The "fields" fixture echoes what it received to stderr, which
        // run_plugin returns. The interface promises no field order, but the
        // BTreeMap makes the host's order name-sorted in practice.
        let lines = run_plugin(inv).expect("plugin should succeed");
        assert_eq!(lines, vec!["audience=world,greeting=hi".to_string()]);
    }

    #[test]
    fn plugin_missing_expected_field_fails() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        // The "fields" fixture requires a `greeting` field; passing none fails.
        assert!(matches!(
            run_plugin(invocation(wasm_path, "fields")),
            Err(RunPluginError::PluginFailed { ref message }) if message == "missing 'greeting' field"
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn dir_and_file_keys_reach_the_plugin() {
        let Some(wasm_path) = option_env!("TEST_PLUGIN_WASM") else {
            return;
        };
        // A real dir and file must exist: the host preopens the dir and reads
        // the file before calling exec().
        let tmp = camino_tempfile::tempdir().expect("create tempdir");
        let base = tmp.path();
        std::fs::create_dir_all(base.join("seeds")).expect("create dir");
        std::fs::write(base.join("cfg.txt"), b"data").expect("write file");

        // The task payload type is irrelevant here: only step output is observed.
        let (reporter, mut rx) = icp_events::step_channel::<()>();
        let result = tokio::task::block_in_place(|| {
            let mut inv = invocation(wasm_path, "keys");
            inv.base_dir = base.to_path_buf();
            inv.dirs = vec![KeyedPath {
                key: Some("assets".to_string()),
                path: "seeds".to_string(),
            }];
            inv.files = vec![KeyedPath {
                key: None,
                path: "cfg.txt".to_string(),
            }];
            inv.reporter = reporter;
            run_plugin(inv)
        });
        let lines = result.expect("plugin should succeed");
        assert_eq!(
            lines,
            vec!["dir assets=seeds".to_string(), "file -=cfg.txt".to_string()],
        );
        // The same lines are emitted live as output events.
        assert!(!output_lines(&mut rx).is_empty());
    }

    #[test]
    fn legacy_v1_plugin_is_detected_and_driven() {
        // A plugin built against the v0.1.0 interface must still load: the host
        // reads its declared interface version and drives it through the v1 path.
        let Some(wasm_path) = option_env!("TEST_PLUGIN_V1_WASM") else {
            return;
        };
        assert!(run_plugin(invocation(wasm_path, "ok")).is_ok());
        // Its error surface flows through the same machinery as v0.2.0 plugins.
        assert!(matches!(
            run_plugin(invocation(wasm_path, "error")),
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
            let mut inv = invocation(wasm_path, "hello");
            inv.reporter = reporter;
            run_plugin(inv)
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
