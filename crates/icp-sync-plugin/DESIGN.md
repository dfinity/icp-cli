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
> - [`sync-plugin.wit`](sync-plugin.wit) — the current interface (v0.2.0), and its source of truth
> - [`sync-plugin-v1.wit`](sync-plugin-v1.wit) — the frozen v0.1.0 interface, still loadable

---

## Interface Design Rationale

The behaviour of the WIT interface is documented for plugin authors in the user
docs; the *reasons* behind those choices are recorded here.

- **`result<T, E>` throughout** — every fallible function returns
  `result<..., string>`, so plugins can use `?` uniformly.
- **Raw Candid bytes at the boundary** — `canister-call-request.arg` is
  `list<u8>`. The plugin owns Candid encoding/decoding; the host forwards bytes
  unchanged. This keeps the host free of any per-canister type knowledge.
- **`canister-call` takes an explicit `target`** — the plugin selects the
  canister being synced (`host`) or a canister it declared as a dependency, by
  name or principal. The host resolves the target and *enforces* the
  declaration: a target absent from the step's `canisters:` list is rejected
  without a call. (In the earlier `@0.1.0` interface `canister-call` had no
  target and always reached the canister being synced; see *Interface
  versioning* below.)
- **`sync-exec-input` carries the canister ID table** — `canister-ids` exposes
  the project's name→principal map for the environment, so a plugin can resolve
  canister names it knows about. It is informational only; calling still
  requires a declaration.
- **Filesystem access via WASI, not a host import** — plugins use standard
  language APIs (`std::fs`); the host preopens the declared `dirs` read-only. No
  bespoke `read-file`/`list-dir` import is needed.
- **Logging via stdio, not a host import** — stdout/stderr are captured by the
  host and forwarded to the CLI. Plugins use normal print facilities.
- **No generated bindings checked in** — `wasmtime::component::bindgen!` (host)
  and `wit_bindgen::generate!` (guest) both run at build time from the WIT files,
  which stay the source of truth for the interface they define.

---

## Crate Structure

### `crates/icp-sync-plugin`

Host-side Component Model runtime for sync plugins.

```
crates/icp-sync-plugin/
  src/
    lib.rs             — public API: run_plugin(), RunPluginError
    runtime.rs         — wasmtime component setup, HostState, bindgen!, exec() call
    path.rs            — declared-path safety checks (escapes_base, symlinks)
  sync-plugin.wit      — current WIT interface, v0.2.0
  sync-plugin-v1.wit   — frozen WIT interface, v0.1.0
  Cargo.toml           — wasmtime, wasmtime-wasi, ic-agent, candid, camino, snafu, tokio, semver
```

Public function:

```rust
pub fn run_plugin(invocation: PluginInvocation) -> Result<Vec<String>, RunPluginError>
```

`PluginInvocation` bundles the inputs: `wasm_path`, `base_dir`, `dirs`, `files`,
`host_canister_id` (the canister being synced), `agent`, `proxy`,
`identity_principal`, `environment`, `compute_limit_secs`, the exposed
`canister_ids` table, the `callable: CallableCanisters` enforcement set, and
`reporter`. The CLI resolves the manifest's declared `canisters:` into
`CallableCanisters` before calling; this crate stays free of any manifest
knowledge.

`dirs` and `files` are the manifest-relative path strings, straight from the
adapter. The runtime owns *all* filesystem access anchored at `base_dir`: it
preopens each `dir` from `base_dir.join(dir)` and reads each `file` from
`base_dir.join(file)`, passing the contents inline in `SyncExecInput`. Keeping
both inside the runtime means the path-safety logic (below) lives in one place
and stays private to this crate — the CLI just forwards strings. The returned
`Vec<String>` is the plugin's persistent stderr lines (see stdio capture below);
`reporter` receives the same lines live, as output events.

### Declared-path safety (no symlinks)

Declared `dirs`/`files` entries are resolved on the host *before* the WASI
sandbox boundary, so a lexical "relative, no `..`" check is not enough on two
counts. First, a Windows drive-relative path such as `C:foo` carries a `Prefix`
component yet is not "absolute", so joining it would discard `base_dir`;
`escapes_base` (in `path.rs`) rejects `..`, root, and drive-prefix components,
mirroring the bundler's checks. Second, a declared entry that *is* a symlink —
or that traverses a symlinked parent component — would let a preopen or a read
resolve outside the canister directory; `first_symlink_component` walks each
component of the declared path under `base_dir` and rejects the entry if any
prefix is a symlink (returning the offending sub-path relative to `base_dir`, so
errors don't leak absolute on-disk paths). Both helpers are crate-private and
applied uniformly to `dirs` and `files`. Symlinks are forbidden outright for
now; the restriction can be relaxed later if a safe use case emerges. (Symlinks
*inside* a preopen that escape it are a separate concern, already rejected by
the WASI sandbox — cap-std — at runtime.)

### `HostState` and bindgen

Both interface versions are bound, each `bindgen!` in its own module so their
generated types don't collide:

```rust
mod v2 { wasmtime::component::bindgen!({ world: "sync-plugin", path: "sync-plugin.wit"    }); }
mod v1 { wasmtime::component::bindgen!({ world: "sync-plugin", path: "sync-plugin-v1.wit" }); }

struct HostState {
    host_canister_id: Principal,
    callable: CallableCanisters,          // by_name + by_id, from the manifest
    agent: Arc<Agent>,
    proxy: Option<Principal>,
    wasi_ctx: wasmtime_wasi::WasiCtx,
    wasi_table: wasmtime_wasi::ResourceTable,
    epoch_extension: Arc<AtomicU64>,
}

// Implemented for both v1::SyncPluginImports and v2::SyncPluginImports; both
// delegate to one shared `do_canister_call(target, ...)`.
```

`HostState` implements `WasiView` so wasmtime_wasi can access the WASI context.
`canister_call` uses `tokio::runtime::Handle::current().block_on(...)` because
the caller already wraps the synchronous `run_plugin` in
`tokio::task::block_in_place`. For a v0.2.0 plugin the target is resolved from
the request's `call-target` by `resolve_call_target`, which enforces the
`callable` set; for a v0.1.0 plugin the target is always `host_canister_id`.
When a proxy is configured and the call is a non-`direct` update, it is encoded
as `ProxyArgs` and routed through the proxy's `proxy` method; otherwise it goes
straight to the resolved target via `ic-agent`.

### Interface versioning (parallel v0.1.0 / v0.2.0 support)

A component built with wit-bindgen imports the interface it `use`s as a
versioned instance — `icp:sync-plugin/types@0.1.0` or `@0.2.0`. `run_plugin`
reads that name off `Component::component_type().imports(...)` and matches the
version with semver caret requirements (`^0.1`, `^0.2`) to pick the ABI, then
instantiates the matching `bindgen!` world and builds the matching
`sync-exec-input`. Reading the plugin's declared metadata is preferred over
trial instantiation: it is unambiguous and needs no throwaway `Store`. A
component with no recognized `icp:sync-plugin/types@<version>` import, or an
unsupported version, is rejected with `UnsupportedInterface`. Both `.wit` files
are checked in; `sync-plugin-v1.wit` is the frozen v0.1.0 contract.

### Compute budget (epoch interruption)

The compute-time limit is enforced with wasmtime's epoch interruption: a
background thread calls `Engine::increment_epoch` once per second, and the store
deadline (`set_epoch_deadline`) bounds pure wasm execution. Because canister
calls block the guest while the host awaits the network, `canister_call` records
the elapsed time and the `epoch_deadline_callback` grants it back via
`epoch_extension` — so network latency is *not* charged against the limit. The
ticker thread stops when its RAII guard drops at the end of `run_plugin`.

The deadline in seconds is the `compute_limit_secs` parameter. The CLI resolves
it from the `ICP_CLI_PLUGIN_COMPUTE_LIMIT_SECS` environment variable, defaulting
to `DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS` (60) when unset.

### stdio capture

`LineCapture` implements `StdoutStream`/`OutputStream`, splits guest output on
newlines, strips ANSI codes, and emits each complete line to the `reporter` as
an output event for the rolling step view. stderr lines are additionally
accumulated and returned from `run_plugin` so the CLI can reprint them
persistently. Each stream is capped at 1 MiB; overflow is dropped and a single
truncation note is emitted on `finalize`.

### `crates/icp/src/manifest/adapter/plugin.rs`

Deserializes the `canister.yaml` fields into:

```rust
pub struct Adapter {
    pub source: SourceField,              // path: or url:
    pub sha256: Option<String>,
    pub dirs: Option<Vec<String>>,
    pub files: Option<Vec<String>>,
    pub canisters: Option<Vec<CanisterRef>>, // extra callable canisters
}
```

`CanisterRef` is an untagged `Principal | Name` (anything that parses as a
principal is one; everything else is a name), written in the manifest as a plain
string. `Deserialize` is hand-written to reject a `url` source without a
`sha256`.

### `crates/icp/src/canister/sync/plugin.rs`

Resolves the wasm (local read or remote HTTP fetch into the package cache),
verifies sha256, builds the exposed canister ID table and the `CallableCanisters`
enforcement set (resolving `canisters:` against the project's IDs), then calls
`icp_sync_plugin::run_plugin(...)` with a `PluginInvocation`. The runtime — not
the CLI — opens the declared paths and enforces the path-safety checks, so the
CLI no longer touches the plugin's input files itself. `exposed_canister_ids`
adds a bare-local-name duplicate for every canister in the same subproject as
the one being synced; `resolve_callable` fails the step if a declared dependency
name does not resolve.
