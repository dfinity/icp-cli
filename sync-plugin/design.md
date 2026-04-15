# Sync Plugin System Design

## Overview

This document describes the design for extending `icp sync` with a new step type:
**`plugin`**. A sync plugin is a WebAssembly component whose `exec()` function is
invoked by `icp-cli` during the sync phase for a specific canister. Plugins run
inside the wasmtime sandbox with deliberately restricted permissions.

---

## Motivation

The existing sync steps (`script` and `assets`) cover common patterns, but
cannot express arbitrary post-deployment logic without shelling out. Shell
scripts lack structure, have unrestricted host access, and cannot be distributed
as self-contained verifiable artifacts.

Sync plugins fill that gap:

- Written in any language that targets WebAssembly (Rust, Go, C, etc.)
- Distributed as a single `.wasm` component file (local or remote URL + sha256)
- Sandboxed — cannot make arbitrary syscalls, network connections, or file
  system access beyond what the host explicitly allows
- Can call canister methods (update and query) on **exactly one canister** —
  the one being synced — via the `canister-call` host function
- Can read files from a declared allowlist of directories via the `read-file`
  host function

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
      dirs:                               # optional read-access directories
        - assets/seed-data/
        - config/

    # Remote plugin (downloaded + verified before execution)
    - type: plugin
      url: https://example.com/plugins/migrate-v2.wasm
      sha256: a665a45920422f9d417e...   # required for remote
```

**Fields**:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | `"plugin"` | yes | Identifies the step type |
| `path` | string | one of `path`/`url` | Local path to the wasm file, relative to canister directory |
| `url` | string | one of `path`/`url` | Remote URL to download the wasm file from |
| `sha256` | string | required for `url`, optional for `path` | SHA-256 hex digest of the wasm file |
| `dirs` | `[string]` | no | Directories (relative to canister dir) the plugin may read from |

---

## Plugin Interface (WIT)

The interface is defined in [sync-plugin.wit](sync-plugin.wit) — that file is the
source of truth. Notable design choices:

- **`result<T, E>` throughout** — all fallible host functions return
  `result<..., string>`, and `exec` returns `result<option<string>, string>`.
  This lets the guest use Rust's `?` operator directly on every host call.

- **No JSON at the boundary** — types are encoded via the Canonical ABI, which
  wasmtime handles transparently. Neither the host nor the plugin deals with
  serialization.

- **`canister-call` takes a request record, not a canister ID** — the host
  always calls the canister from `sync-exec-input.canister-id`; the plugin
  cannot supply a different target. The restriction is structural, not enforced
  by a runtime check on a field value.

---

## Host-Side Enforcement

The host functions registered via `wasmtime::component::bindgen!` enforce all
restrictions through the host state struct — there is no way for the wasm
component to bypass them:

### `canister-call`

```
Captured: target_canister_id: Principal
Enforcement: always calls target_canister_id regardless of plugin request;
             plugin cannot call any other principal
```

### `read-file`

```
Captured: allowed_dirs: Vec<Utf8PathBuf>  (absolute, canonicalized)
Enforcement: canonicalize(requested_path) must have one of allowed_dirs as a prefix
             → if not, return Err(...) to the plugin
```

### `list-dir`

```
Captured: allowed_dirs: Vec<Utf8PathBuf>  (absolute, canonicalized)
Enforcement: same prefix check as read-file
Result: entries one level deep (name + is-dir flag); caller descends by
        calling list-dir again with an appended entry name
```

Canonicalization prevents `../` traversal attacks for both `read-file` and
`list-dir`.

### `log`

No restrictions — prints to the CLI progress stream (or stdout during testing).

### Network / other I/O

The wasmtime Component Model sandbox does not expose WASI socket or filesystem
interfaces to the component unless explicitly linked. Since the host only links
the four declared import functions, the plugin cannot open sockets, write files,
or spawn processes.

---

## Crate Structure

### `crates/icp-sync-plugin`

Runtime crate — host-side Component Model integration for sync plugins.

```
crates/icp-sync-plugin/
  src/
    lib.rs          — public API: run_plugin(...), RunPluginError
    runtime.rs      — wasmtime component setup, host state, bindgen!, exec() call
    sandbox.rs      — path canonicalization + allowlist enforcement
  Cargo.toml        — depends on: wasmtime (component-model feature), candid,
                      candid-parser, ic-agent, camino, snafu, tokio
```

Public function signature:

```rust
pub fn run_plugin(
    wasm_path: Utf8PathBuf,
    base_dir: Utf8PathBuf,
    allowed_dirs: Vec<Utf8PathBuf>,
    target_canister_id: Principal,
    agent: Agent,
    environment: String,
    stdio: Option<Sender<String>>,
) -> Result<(), RunPluginError>
```

### Host-Side Pattern (`runtime.rs`)

```rust
wasmtime::component::bindgen!({
    world: "sync-plugin",
    path: "../../sync-plugin/sync-plugin.wit",
});

struct HostState { /* target_canister_id, agent, allowed_dirs, base_dir, stdio */ }

impl SyncPluginImports for HostState {
    fn canister_call(&mut self, req: CanisterCallRequest) -> Result<String, String> { ... }
    fn read_file(&mut self, path: String) -> Result<String, String> { ... }
    fn list_dir(&mut self, path: String) -> Result<Vec<DirEntry>, String> { ... }
    fn log(&mut self, message: String) { ... }
}

// In run_plugin:
let engine = Engine::new(Config::new().wasm_component_model(true))?;
let component = Component::from_file(&engine, &wasm_path)?;
let mut store = Store::new(&engine, host_state);
let (plugin, _) = SyncPlugin::instantiate(&mut store, &component, &linker)?;
let result = plugin.call_exec(&mut store, &input)?;
```

The `bindgen!` macro generates `SyncPlugin`, `SyncPluginImports`, and all WIT
types as plain Rust structs/enums — no JSON, no manual serialization.

### `crates/icp/src/manifest/adapter/plugin.rs`

Describes the `canister.yaml` fields:

```rust
pub struct Adapter {
    #[serde(flatten)]
    pub source: super::prebuilt::SourceField,
    pub sha256: Option<String>,
    pub dirs: Option<Vec<String>>,
}
```

### `crates/icp/src/canister/sync/plugin.rs`

Resolves the wasm, verifies sha256, canonicalizes dirs, then calls
`icp_sync_plugin::run_plugin(...)`.

---

## Writing a Sync Plugin (Rust)

Plugins are built as WebAssembly components targeting `wasm32-wasip2` using
[`cargo component`](https://github.com/bytecodealliance/cargo-component):

```bash
cargo install cargo-component
cargo component build --release
```

The WIT file (`sync-plugin/sync-plugin.wit`) is distributed with the tool and
referenced in the plugin's `Cargo.toml`:

```toml
[package.metadata.component]
package = "icp:sync-plugin"
```

**`src/lib.rs`** — implement the generated `Guest` trait:

```rust
cargo_component_bindings::generate!();

use bindings::Guest;
use bindings::icp::sync_plugin::types::{CanisterCallRequest, CallType, SyncExecInput};

struct MyPlugin;

impl Guest for MyPlugin {
    fn exec(input: SyncExecInput) -> Result<Option<String>, String> {
        bindings::log(&format!("syncing canister {}", input.canister_id));

        let entries = bindings::list_dir("seed-data/")?;

        for entry in entries {
            if entry.is_dir { continue; }

            let path = format!("seed-data/{}", entry.name);
            let data = bindings::read_file(&path)?;

            bindings::canister_call(CanisterCallRequest {
                method: "seed".to_string(),
                arg: format!("(\"{}\")", data.trim()),
                call_type: Some(CallType::Update),
            })?;

            bindings::log(&format!("{path}: ok"));
        }

        Ok(Some(format!(
            "seeded canister {} in environment {}",
            input.canister_id, input.environment
        )))
    }
}
```

`cargo_component_bindings::generate!()` runs at build time — nothing generated
is committed to the repo. The WIT file is the sole source of truth.

---

## Sandbox Summary

| Capability | Allowed | Enforcement |
|------------|---------|-------------|
| `canister-call` to target canister | Yes | Host always uses captured `target_canister_id` |
| `canister-call` to any other canister | No | Not a parameter; host ignores any such intent |
| `read-file` within declared `dirs` | Yes | Path allowlist checked after canonicalization |
| `read-file` outside declared `dirs` | No | Returns `Err(...)` to plugin |
| `list-dir` within declared `dirs` | Yes | Path allowlist checked after canonicalization |
| `list-dir` outside declared `dirs` | No | Returns `Err(...)` to plugin |
| `log` (print to CLI output) | Yes | Unrestricted |
| Arbitrary filesystem write | No | No WASI filesystem write interface linked |
| Network access (TCP/UDP/etc.) | No | No WASI socket interface linked |
| Spawning processes | No | No WASI process interface linked |
| Calls to other environments | No | Agent scoped to environment at plugin load time |

---

## Decisions

**1. No generated file checked in**

`wasmtime::component::bindgen!` (host side) and `cargo_component_bindings::generate!()`
(guest side) both run at build time — nothing generated is committed to the repo.
The WIT file is the sole source of truth.

**2. `result<T, E>` for all fallible functions**

`exec` returns `result<option<string>, string>` — the ok arm carries optional
output text, the err arm carries the error message. All host functions follow the
same pattern, so the guest can use `?` uniformly.

**3. `dirs` resolution**

Relative to the canister directory. Consistent with other adapters.

**4. Caching downloaded wasm**

Not implemented in the POC — deferred.

**5. Plugin timeout**

Not implemented in the POC. wasmtime supports epoch-based interruption and
fuel-based metering; adding a configurable `timeout_seconds` field to the
adapter is a follow-up.

---

## Follow-up Items

- **Wasm caching**: cache remote plugin wasm files in `.icp/cache/`.
- **Plugin timeout**: add `timeout_seconds: Option<u64>` to
  `adapter::plugin::Adapter`; wire through to wasmtime epoch interruption.
