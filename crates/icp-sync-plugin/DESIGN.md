# Sync Plugin System Design

This crate is the **host-side runtime** for sync plugins: it loads a plugin
WebAssembly component inside a [wasmtime](https://wasmtime.dev/) WASI sandbox and
invokes its `exec()` export during `icp sync` for a single canister.

> **User-facing documentation lives in the main docs** — start there for the
> motivation, the manifest syntax, the plugin interface, the sandbox model, and
> how to write a plugin. This file covers only the host implementation and the
> design rationale that those docs do not.
>
> - [Sync Plugins](../../docs/concepts/sync-plugins.md) — concept, WIT interface, sandbox, resource limits
> - [Writing a Sync Plugin](../../docs/guides/writing-sync-plugins.md) — authoring guide (Rust)
> - [Plugin Sync (Configuration Reference)](../../docs/reference/configuration.md) — `type: plugin` manifest fields
> - [`sync-plugin.wit`](sync-plugin.wit) — the interface, and the sole source of truth

---

## Interface Design Rationale

The behaviour of the WIT interface is documented for plugin authors in the user
docs; the *reasons* behind those choices are recorded here.

- **`result<T, E>` throughout** — every fallible function returns
  `result<..., string>`, so plugins can use `?` uniformly.
- **Raw Candid bytes at the boundary** — `canister-call-request.arg` is
  `list<u8>`. The plugin owns Candid encoding/decoding; the host forwards bytes
  unchanged. This keeps the host free of any per-canister type knowledge.
- **`canister-call` takes no canister ID** — the host always calls the canister
  from `sync-exec-input.canister-id`. There is deliberately no field for a
  different target, so the single-canister restriction is *structural* rather
  than a policy the plugin could bypass.
- **Filesystem access via WASI, not a host import** — plugins use standard
  language APIs (`std::fs`); the host preopens the declared `dirs` read-only. No
  bespoke `read-file`/`list-dir` import is needed.
- **Logging via stdio, not a host import** — stdout/stderr are captured by the
  host and forwarded to the CLI. Plugins use normal print facilities.
- **No generated bindings checked in** — `wasmtime::component::bindgen!` (host)
  and `wit_bindgen::generate!` (guest) both run at build time from the WIT file,
  which stays the single source of truth.

---

## Crate Structure

### `crates/icp-sync-plugin`

Host-side Component Model runtime for sync plugins.

```
crates/icp-sync-plugin/
  src/
    lib.rs          — public API: run_plugin(), RunPluginError
    runtime.rs      — wasmtime component setup, HostState, bindgen!, exec() call
  sync-plugin.wit   — WIT interface (source of truth)
  Cargo.toml        — wasmtime, wasmtime-wasi, ic-agent, candid, camino, snafu, tokio
```

Public function:

```rust
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
) -> Result<Vec<String>, RunPluginError>
```

`dirs` and `files` come directly from the manifest adapter. The runtime preopens
each `dir` from `base_dir.join(dir)` and passes `files` inline in
`SyncExecInput`. The returned `Vec<String>` is the plugin's persistent stderr
lines (see stdio capture below); `stdio`, when set, receives the rolling
progress lines live.

### Declared-path safety (no symlinks)

Declared `dirs`/`files` entries are resolved on the host *before* the WASI
sandbox boundary, so the lexical "relative, no `..`" check is not enough: a
declared entry that *is* a symlink — or that traverses a symlinked parent
component — would let a preopen or a host read resolve to a target outside the
canister directory. `first_symlink_component` (in `path.rs`, shared with the
CLI's `files` reader) walks each component of the declared path under
`base_dir` and rejects the entry if any prefix is a symlink. Symlinks are
forbidden outright for now; the restriction can be relaxed later if a safe use
case emerges. (Symlinks *inside* a preopen that escape it are a separate
concern, already rejected by the WASI sandbox — cap-std — at runtime.)

### `HostState` and bindgen

```rust
wasmtime::component::bindgen!({
    world: "sync-plugin",
    path: "sync-plugin.wit",
});

struct HostState {
    target_canister_id: Principal,
    agent: Arc<Agent>,
    proxy: Option<Principal>,
    wasi_ctx: wasmtime_wasi::WasiCtx,
    wasi_table: wasmtime_wasi::ResourceTable,
    epoch_extension: Arc<AtomicU64>,
}

impl SyncPluginImports for HostState {
    fn canister_call(&mut self, req: CanisterCallRequest) -> Result<Vec<u8>, String> { ... }
}
```

`HostState` implements `WasiView` so wasmtime_wasi can access the WASI context.
`canister_call` uses `tokio::runtime::Handle::current().block_on(...)` because
the caller already wraps the synchronous `run_plugin` in
`tokio::task::block_in_place`. When a proxy is configured and the call is a
non-`direct` update, it is encoded as `ProxyArgs` and routed through the proxy's
`proxy` method; otherwise it goes straight to the target via `ic-agent`.

### Compute budget (epoch interruption)

The compute-time limit is enforced with wasmtime's epoch interruption: a
background thread calls `Engine::increment_epoch` once per second, and the store
deadline (`set_epoch_deadline`) bounds pure wasm execution. Because canister
calls block the guest while the host awaits the network, `canister_call` records
the elapsed time and the `epoch_deadline_callback` grants it back via
`epoch_extension` — so network latency is *not* charged against the limit. The
ticker thread stops when its RAII guard drops at the end of `run_plugin`.

### stdio capture

`LineCapture` implements `StdoutStream`/`OutputStream`, splits guest output on
newlines, strips ANSI codes, and (best-effort) forwards each complete line to
the `stdio` channel for the rolling step view. stderr lines are additionally
accumulated and returned from `run_plugin` so the CLI can reprint them
persistently. Each stream is capped at 1 MiB; overflow is dropped and a single
truncation note is emitted on `finalize`.

### `crates/icp/src/manifest/adapter/plugin.rs`

Deserializes the `canister.yaml` fields into:

```rust
pub struct Adapter {
    pub source: SourceField,         // path: or url:
    pub sha256: Option<String>,
    pub dirs: Option<Vec<String>>,
    pub files: Option<Vec<String>>,
}
```

`Deserialize` is hand-written to reject a `url` source without a `sha256`.

### `crates/icp/src/canister/sync/plugin.rs`

Resolves the wasm (local read or remote HTTP fetch into the package cache),
verifies sha256, reads the inline `files` (rejecting absolute, `..`, or
symlinked paths via `first_symlink_component`), then calls
`icp_sync_plugin::run_plugin(...)`.
