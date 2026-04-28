# Sync Plugin System Design

## Overview

Sync plugins extend `icp sync` with an arbitrary post-deployment step type.
A plugin is a WebAssembly component whose `exec()` export is invoked by
`icp-cli` during sync for a specific canister. The host runs it inside a
[wasmtime](https://wasmtime.dev/) WASI sandbox with a deliberately narrow
capability surface.

---

## Motivation

The existing sync steps (`script` and `assets`) cover common patterns but
cannot express arbitrary post-deployment logic without shelling out. Shell
scripts lack structure, have unrestricted host access, and cannot be
distributed as self-contained verifiable artifacts.

Sync plugins fill that gap:

- Written in any language that compiles to `wasm32-wasip2`
- Distributed as a single `.wasm` component (local path or remote URL + sha256)
- Sandboxed — cannot make arbitrary syscalls, network connections, or
  unrestricted filesystem access
- Can call canister methods (update and query) on **exactly one canister** —
  the one being synced
- Can read files from declared directories via the WASI filesystem interface

---

## Canister Manifest Syntax

A sync plugin step is declared in `canister.yaml` under `sync.steps` with
`type: plugin`:

```yaml
name: my-canister
build:
  steps:
    - type: pre-built
      path: dist/my_canister.wasm

sync:
  steps:
    # Local plugin
    - type: plugin
      path: ./plugins/populate-data.wasm
      sha256: e3b0c44298fc1c149afb...   # optional but recommended
      dirs:                               # directories preopened read-only
        - assets/seed-data
        - config
      files:                             # files read by the host and passed inline
        - config.txt

    # Remote plugin (downloaded + verified before execution)
    - type: plugin
      url: https://example.com/plugins/migrate-v2.wasm
      sha256: a665a45920422f9d417e...   # required for remote
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | `"plugin"` | yes | Identifies the step type |
| `path` | string | one of `path`/`url` | Local path to the wasm, relative to canister directory |
| `url` | string | one of `path`/`url` | Remote URL to download the wasm from |
| `sha256` | string | required for `url`, optional for `path` | SHA-256 hex digest of the wasm file |
| `dirs` | `[string]` | no | Directories (relative to canister dir) the plugin may read; each is preopened via WASI |
| `files` | `[string]` | no | Files (relative to canister dir) read by the host and passed inline in `sync-exec-input.files` |

---

## Plugin Interface (WIT)

The interface is defined in [`sync-plugin.wit`](sync-plugin.wit) — that file
is the source of truth. The world has one host-provided import and one
plugin-provided export:

```wit
world sync-plugin {
    // Host import: call the canister being synced.
    import canister-call: func(req: canister-call-request) -> result<list<u8>, string>;

    // Plugin export: run the sync step.
    export exec: func(input: sync-exec-input) -> result<option<string>, string>;
}
```

Notable choices:

- **`result<T, E>` throughout** — all fallible functions return
  `result<..., string>`, so plugins can use `?` uniformly.
- **Raw Candid bytes at the boundary** — `canister-call-request.arg` is
  `list<u8>`. The plugin encodes the argument (e.g. with `candid::Encode!`)
  and the host forwards bytes unchanged. The response is also raw bytes for
  the plugin to decode.
- **`canister-call` takes no canister ID** — the host always calls the
  canister from `sync-exec-input.canister-id`. The plugin cannot supply a
  different target; the restriction is structural.
- **Filesystem access via WASI, not a host import** — plugins use standard
  language APIs (`std::fs` in Rust). The host preopens the declared `dirs`
  read-only; no explicit `read-file` or `list-dir` import is needed.
- **Logging via stdio, not a host import** — stdout and stderr are captured
  by the host (via `MemoryOutputPipe`) and forwarded to the CLI's progress
  output after `exec()` returns. Plugins use normal print facilities.
- **No generated files checked in** — `wasmtime::component::bindgen!` (host)
  and `wit_bindgen::generate!` (guest) both run at build time from the WIT
  file. The WIT is the sole source of truth.

---

## Sandbox

### Filesystem

- The host preopens each directory listed in `dirs:` **read-only**
  (`DirPerms::READ`, `FilePerms::READ`) via `WasiCtxBuilder::preopened_dir`.
- The plugin sees each preopen at the same relative path it used in the
  manifest (e.g. `dirs: ["assets"]` is visible as `assets/` inside the guest).
- Files listed in `files:` are read by the host before plugin execution and
  passed inline in `sync-exec-input.files`. The plugin accesses them from the
  input struct, not from the filesystem.
- Any path not covered by a preopen is invisible. Writes, creates, deletes,
  renames, and symlinks that escape a preopen are rejected by wasmtime.

### WASI capabilities

The host links `wasi:cli/imports` via `wasmtime_wasi::p2::add_to_linker_sync`.
The effective capability surface is:

| Capability | Available | Notes |
|------------|-----------|-------|
| `wasi:filesystem` | read-only preopens | constrained to declared `dirs` |
| `wasi:io`, `wasi:clocks`, `wasi:random` | yes | Rust's `HashMap`, `chrono`, etc. work normally |
| `wasi:cli/exit` | yes | `process::exit` / panics abort the guest cleanly |
| `wasi:cli/environment` | empty | returns empty env and args; use `sync-exec-input.environment` |
| `wasi:cli/terminal-*` | not a terminal | color auto-detection libraries simply disable color |
| `wasi:sockets` | blocked | all addresses denied; treat network as unavailable |
| Arbitrary filesystem write | blocked | no writable preopens |
| Spawning subprocesses | blocked | no WASI process interface linked |
| Calls to other canisters | blocked | host ignores any canister ID; always calls the synced canister |

**Stdio:**
- `stdin` is closed.
- `stdout` and `stderr` are captured with `MemoryOutputPipe`. After `exec()`
  returns, stdout is forwarded to the CLI progress output first, then stderr.
  Invalid UTF-8 is replaced with U+FFFD.

### What this means for plugin authors

You can:
- Read any file under a declared `dirs:` entry using standard filesystem APIs.
- Access inline file content from `sync-exec-input.files`.
- Use clocks, RNG, and standard language features.
- Panic or exit — the host surfaces the error and continues.

You cannot:
- Open network connections or resolve DNS.
- Write to disk, spawn subprocesses, or read environment variables.
- Call canisters other than the one being synced.
- Escape a preopen via `..` or symlinks.

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
) -> Result<(), RunPluginError>
```

`dirs` and `files` come directly from the manifest adapter. The runtime
preopens each `dir` from `base_dir.join(dir)` and passes `files` inline in
`SyncExecInput`.

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
}

impl SyncPluginImports for HostState {
    fn canister_call(&mut self, req: CanisterCallRequest) -> Result<Vec<u8>, String> { ... }
}
```

`HostState` implements `WasiView` so wasmtime_wasi can access the WASI context.
`canister_call` uses `tokio::runtime::Handle::current().block_on(...)` because
the caller already wraps the synchronous `run_plugin` in
`tokio::task::block_in_place`.

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

### `crates/icp/src/canister/sync/plugin.rs`

Resolves the wasm (local read or remote HTTP fetch), verifies sha256, reads
inline files, then calls `icp_sync_plugin::run_plugin(...)`.

---

## Writing a Sync Plugin (Rust)

Plugins target `wasm32-wasip2` and use `wit_bindgen::generate!` to produce
bindings from the WIT file at build time:

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
candid = "0.10"
wit-bindgen = { version = "0.56", features = ["realloc"] }
```

```rust
// src/lib.rs
wit_bindgen::generate!({
    world: "sync-plugin",
    path: "../../../crates/icp-sync-plugin/sync-plugin.wit",
});

use candid::Encode;
struct Plugin;

impl Guest for Plugin {
    fn exec(input: SyncExecInput) -> Result<Option<String>, String> {
        // Access inline files from the manifest's `files:` list.
        if let Some(f) = input.files.first() {
            let arg = Encode!(&f.content.trim())
                .map_err(|e| format!("encode error: {e}"))?;
            canister_call(&CanisterCallRequest {
                method: "set_config".to_string(),
                arg,
                call_type: icp::sync_plugin::types::CallType::Update,
                direct: false,
                cycles: 0,
            })?;
        }

        // Access declared directories via standard std::fs.
        for dir in &input.dirs {
            // std::fs::read_dir(dir), etc.
        }

        Ok(Some(format!("done for canister {}", input.canister_id)))
    }
}

export!(Plugin);
```

Build:

```bash
rustup target add wasm32-wasip2
cargo build --target wasm32-wasip2 --release
```

The output `.wasm` file is loaded directly by the host — no additional
tooling is required. See `examples/icp-sync-plugin/` for a working example.
