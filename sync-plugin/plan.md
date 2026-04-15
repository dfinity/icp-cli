# Sync Plugin Implementation Plan

Reference: [sync-plugin/design.md](sync-plugin/design.md)

---

## Step 1 — Create the sync plugin manifest adapter

**New file**: `crates/icp/src/manifest/adapter/plugin.rs`

```rust
use super::prebuilt::SourceField;
use crate::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Configuration for a sync plugin step.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Adapter {
    #[serde(flatten)]
    pub source: SourceField,          // path: or url:
    pub sha256: Option<String>,
    pub dirs: Option<Vec<String>>,    // read-access directory allowlist
}
```

Add `pub mod plugin;` to `crates/icp/src/manifest/adapter/mod.rs`.

---

## Step 2 — Add `SyncStep::Plugin` to the canister manifest

**File**: `crates/icp/src/manifest/canister.rs`

```rust
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum SyncStep {
    Script(adapter::script::Adapter),
    Assets(adapter::assets::Adapter),
    Plugin(adapter::plugin::Adapter),  // NEW
}
```

Update `SyncStep::fmt` to cover the new variant.

Add a test case for the new YAML syntax in `canister.rs` tests:

```yaml
sync:
  steps:
    - type: plugin
      path: ./plugins/my-sync.wasm
      dirs:
        - assets/seed-data/
```

---

## Step 3 — Write the WIT interface file

**File**: `sync-plugin/sync-plugin.wit` (already created)

The WIT world defines the complete contract between icp-cli and any sync plugin.
It uses:
- `result<T, E>` for all fallible operations — no `nullable` field workarounds
- `option<T>` for optional values
- Plain `record` and `enum` types that map directly to Rust structs/enums

The WIT file is the single source of truth for both the host runtime and guest
plugin code — no separate schema file, no generated file checked in.

Add the WIT file path to the workspace `Cargo.toml` as a note for reviewers, or
document it in the crate README. The `bindgen!` macro on the host side and the
`cargo component` tool on the guest side both resolve the path at build time.

---

## Step 4 — Implement `crates/icp-sync-plugin` with wasmtime

**Crate**: `crates/icp-sync-plugin/`

Add to `Cargo.toml`:

```toml
[dependencies]
camino.workspace = true
candid.workspace = true
candid_parser.workspace = true
hex.workspace = true
ic-agent.workspace = true
snafu.workspace = true
tokio.workspace = true
wasmtime = { workspace = true }
```

Add `wasmtime` to the root `Cargo.toml` `[workspace.dependencies]` table with
its version and required features:

```toml
# root Cargo.toml
[workspace.dependencies]
wasmtime = { version = "X", features = ["component-model"] }
```

In `crates/icp-sync-plugin/Cargo.toml` declare it without a version
(`workspace = true` inherits everything from the root).

### `src/sandbox.rs`

Already implemented and tested — no changes needed.

```rust
/// Returns true iff `path` (canonicalized) starts with one of `allowed_dirs`.
pub fn is_path_allowed(path: &Utf8Path, allowed_dirs: &[Utf8PathBuf]) -> bool
```

### `src/runtime.rs`

Replace the stub with the wasmtime Component Model implementation.

```rust
wasmtime::component::bindgen!({
    world: "sync-plugin",
    path: "../../sync-plugin/sync-plugin.wit",
});

struct HostState {
    target_canister_id: Principal,
    agent: Arc<Agent>,
    allowed_dirs: Arc<Vec<Utf8PathBuf>>,
    base_dir: Arc<Utf8PathBuf>,
    stdio: Option<Sender<String>>,
}

impl SyncPluginImports for HostState {
    fn canister_call(&mut self, req: CanisterCallRequest) -> Result<String, String> { ... }
    fn read_file(&mut self, path: String) -> Result<String, String> { ... }
    fn list_dir(&mut self, path: String) -> Result<Vec<DirEntry>, String> { ... }
    fn log(&mut self, message: String) { ... }
}
```

Error variants (one per primary action):

- `LoadComponent { path }` — wasmtime fails to load or parse the component
- `Instantiate { path }` — linker or store setup failure
- `CallExec { path }` — wasmtime trap or ABI error during the exec() call
- `PluginFailed { message }` — exec() returned `Err(message)`

`canister_call` in `HostState` blocks the current thread on the async agent call
using `tokio::runtime::Handle::current().block_on(...)` — the host is already
inside a `tokio::task::block_in_place` call in `sync/plugin.rs`.

### `src/lib.rs`

Re-exports `run_plugin` and `RunPluginError` — no change to the public API.

---

## Step 5 — Implement `sync/plugin.rs` in the `icp` crate

**File**: `crates/icp/src/canister/sync/plugin.rs` (already exists as a stub)

```rust
pub async fn sync(
    adapter: &adapter::plugin::Adapter,
    params: &Params,
    agent: &Agent,
    environment: &str,
    stdio: Option<Sender<String>>,
) -> Result<(), PluginError>
```

Responsibilities:
1. Resolve the wasm path:
   - `Local`: join with `params.path` (canister directory)
   - `Remote`: download to temp file (reuse the download + sha256 utility used
     by the prebuilt build adapter)
2. Verify sha256 if present
3. Canonicalize declared `dirs` relative to `params.path`
4. Call `icp_sync_plugin::run_plugin(...)`

Add `PluginError` variants for each failing action (wasm resolution, download,
sha256 mismatch, run).

---

## Step 6 — Wire `SyncStep::Plugin` into the dispatcher

**File**: `crates/icp/src/canister/sync/mod.rs`

```rust
mod plugin;

// In Syncer::sync():
SyncStep::Plugin(adapter) => {
    Ok(plugin::sync(adapter, params, agent, environment, stdio).await?)
}
```

Add `Plugin` variant to `SynchronizeError`.

The `environment` string must be threaded through from `Params` (add a field)
or passed as a separate parameter — check how `assets::sync` currently receives
it and be consistent.

---

## Step 7 — Build the proof-of-concept plugin

**Directory**: `sync-plugin/poc/`

A Rust wasm plugin that:
1. Lists a declared directory and reads each text file found
2. Calls an update method on the canister, passing the file content as a string argument
3. Logs the result of each call

### Toolchain

Plugins use plain `cargo build` — no `cargo-component` tool required:

```bash
rustup target add wasm32-wasip2
cargo build --target wasm32-wasip2 --release
```

The output is a WebAssembly component binary (`.wasm`) that the host loads
directly with `wasmtime::component::Component::from_file`.

### `Cargo.toml`

```toml
[package]
name = "icp-sync-plugin-poc"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
wit-bindgen = { version = "X", features = ["realloc"] }

[build-dependencies]
# none — build.rs only emits rerun-if-changed directives
```

### `build.rs`

A minimal build script that tells Cargo to re-run bindings generation whenever
the WIT file changes:

```rust
fn main() {
    println!("cargo:rerun-if-changed=../../sync-plugin/sync-plugin.wit");
}
```

### `src/lib.rs`

Use the `wit_bindgen::generate!` proc macro (no separate `build.rs` code
generation step — the macro expands at compile time from the WIT path):

```rust
wit_bindgen::generate!({
    world: "sync-plugin",
    path: "../../sync-plugin/sync-plugin.wit",
});

use exports::icp::sync_plugin::types::{CanisterCallRequest, CallType, GuestExec, SyncExecInput};

struct Plugin;

impl GuestExec for Plugin {
    fn exec(input: SyncExecInput) -> Result<Option<String>, String> {
        log(&format!("sync plugin: starting for canister {}", input.canister_id));

        let entries = list_dir("seed-data/")?;

        for entry in entries {
            if entry.is_dir { continue; }
            let path = format!("seed-data/{}", entry.name);
            let data = read_file(&path)?;
            canister_call(CanisterCallRequest {
                method: "seed".to_string(),
                arg: format!("(\"{}\")", data.trim().replace('"', "\\\"")),
                call_type: Some(CallType::Update),
            })?;
            log(&format!("{path}: ok"));
        }

        Ok(Some(format!(
            "seeded canister {} in environment {}",
            input.canister_id, input.environment
        )))
    }
}

export!(Plugin);
```

---

## Step 8 — Update JSON schema and CLI docs

```bash
./scripts/generate-config-schemas.sh   # regenerate canister-yaml-schema.json
./scripts/generate-cli-docs.sh         # regenerate CLI reference docs
```

The new `SyncStep::Plugin` variant and `adapter::plugin::Adapter` implement
`JsonSchema` (via `schemars`), so the schema generator picks them up
automatically once wired in.

---

## Step 9 — Add integration tests

- A `canister.yaml` fixture with `type: plugin` in `crates/icp-cli/tests/` or
  `examples/`
- Unit tests in `adapter/plugin.rs` (YAML round-trip, same style as
  `adapter/prebuilt.rs`)
- Unit tests in `sync/plugin.rs` for sha256 verification and path allowlist
  enforcement (no network needed — use a minimal hand-crafted wasm component or
  build the poc plugin in the test)
- Unit tests in `sandbox.rs` for `list_dir` allowlist enforcement: path outside
  allowed dirs, `../` traversal attempts, and a valid listing (these already
  exist and pass)

---

## Order of Dependencies

```
Step 1 (plugin adapter) ──► Step 2 (SyncStep::Plugin)
                                 └─ Step 6 (dispatcher)
Step 3 (WIT file — already done)
Step 4 (icp-sync-plugin runtime)
  └─ Step 5 (sync/plugin.rs) ──► Step 6 (dispatcher)
Step 8 (schema + docs) — after Steps 1–2
Step 9 (tests) — after Steps 1–6
```

Steps 1–2 (manifest layer) and Step 4 (runtime layer) can be developed
independently and in parallel. Step 5 joins both. Step 6 is the final wire-up.

---

## Follow-up Items (post-POC)

These are out of scope for the current implementation; tracked here for later:

- **Wasm caching**: cache remote plugin wasm in `.icp/cache/` to avoid
  re-downloading on every sync.
- **Plugin timeout**: add `timeout_seconds: Option<u64>` to
  `adapter::plugin::Adapter` and wire through to wasmtime's epoch interruption
  mechanism.
