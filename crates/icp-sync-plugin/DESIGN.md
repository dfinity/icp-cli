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
  canister being synced (`host`) or a canister from the step's `canisters:`
  list, by name. The host resolves the target and *enforces* the list: a target
  absent from it is rejected without a call. Names are the only way to address
  another canister: the name→principal mapping is the host's to make, since it
  varies per environment, and a plugin that hardcodes a principal is pinned to
  one deployment. (In the earlier `@0.1.0` interface
  `canister-call` had no target and always reached the canister being synced; see
  *Interface versioning* below.)
- **`get-metadata-section` mirrors `canister-call`'s targeting and routing** — it
  takes the same `call-target` (enforced against `canisters:` the same way) and
  the same `direct` flag, so one mental model covers both imports. Its return is
  `result<option<list<u8>>, string>`: a missing section is an ordinary answer for
  a plugin probing for an optional section, not a failure it must recognize by
  parsing error text. The host pays for that guarantee on the proxied path — see
  *Metadata reads* below.
- **`sync-exec-input` carries the canister ID table** — `canister-ids` exposes
  the project's name→principal map for the environment, so a plugin can resolve
  canister names it knows about. It is informational only; calling still
  requires an entry in `canisters:`.
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
    path.rs            — declared-path resolution and safety checks (project bound, symlinks)
  sync-plugin.wit      — current WIT interface, v0.2.0
  sync-plugin-v1.wit   — frozen WIT interface, v0.1.0
  Cargo.toml           — wasmtime, wasmtime-wasi, ic-agent, ic-management-canister-types,
                         candid, camino, snafu, tokio, semver
```

Public function:

```rust
pub fn run_plugin(invocation: PluginInvocation) -> Result<Vec<String>, RunPluginError>
```

`PluginInvocation` bundles the inputs: `wasm_path`, `base_dir`, `project_dir`,
`dirs`, `files`, `fields`, `host_canister_id` (the canister being synced),
`agent`, `proxy`, `identity_principal`, `environment`, `compute_limit_secs`, the
exposed `canister_ids` table, the `callable: CallableCanisters` enforcement set,
and `stdio`. The CLI resolves the manifest's declared `canisters:` into
`CallableCanisters` before calling; this crate stays free of any manifest
knowledge.

`dirs` and `files` are the manifest-relative paths (as `KeyedPath`s carrying the
map key each was declared under, if any), straight from the adapter. The runtime
owns *all* filesystem access: it resolves each entry against `base_dir`,
preopens each `dir` and reads each `file` from the location that resolves to,
passing the contents — and the keys — inline in `SyncExecInput`. Keeping both
inside the runtime means the path-safety logic (below) lives in one place and
stays private to this crate — the CLI just forwards strings. The returned
`Vec<String>` is the plugin's persistent stderr lines (see stdio capture below);
`stdio`, when set, receives the rolling progress lines live.

### Declared-path safety (project-bounded, no symlinks)

An entry is written relative to `base_dir` (the canister directory) but bounded
by `project_dir`: it may rise out of the canister directory with `..` and reach
anything else in the project, and nothing above the project. `path.rs` resolves
one against the other:

- `base_within_root` places `base_dir` inside `project_dir` as a clean component
  list. When `base_dir` does not lie within it — a dependency project reached by
  an out-of-tree `path:`, which `icp project bundle` rejects but `icp sync`
  allows — there is no project-relative position to anchor at, so `base_dir`
  becomes its own root: exactly the rule that predated the widening, and no
  narrower than what such a project could already reach. (This is a fallback for
  an unanchorable base, not a tighter grant for dependencies. A dependency
  vendored inside the workspace is bounded by the workspace root like any other
  canister, and its manifest can in any case run arbitrary commands through a
  `script` step.)
- `resolve` walks the declared entry from there, resolving `.`/`..` lexically. A
  `..` with nothing left to pop is `Escape::AboveRoot`; a root or drive-prefix
  component is `Escape::NotRelative` — a Windows drive-relative path such as
  `C:foo` carries a `Prefix` component yet is not "absolute", so joining it
  would discard the base. This mirrors the bundler's checks.
- The host path is the *resolved* location joined onto the root, never the
  declared path joined onto `base_dir`: the latter would leave a `..` for the OS
  to resolve through whatever `base_dir`'s own components happen to be.
- `Resolved::first_symlink_component` then walks the resolved path under the
  root and rejects the entry if any component is a symlink (returning the
  offending sub-path relative to the root, so errors don't leak absolute on-disk
  paths). An entry that stays below `base_dir` is checked only from there down —
  the ancestry reaching the canister directory is exempt on the same grounds as
  the root itself, since how the project reaches its own canister is not
  something a manifest declared. An entry that rises *out* of `base_dir` is
  checked from the root down instead: it re-anchors on an ancestor and descends
  where the canister directory's own path never went, so a symlink in that
  ancestry would put its target outside the project. An entry that *is* a
  symlink, or that traverses one,
  would otherwise let a preopen or a read resolve outside the project. Symlinks
  are forbidden outright for now; the restriction can be relaxed later if a safe
  use case emerges. (Symlinks *inside* a preopen that escape it are a separate
  concern, already rejected by the WASI sandbox — cap-std — at runtime.)

The guest still sees each preopen under the path the manifest wrote, `..` and
all, so a plugin opens `dir.path` verbatim regardless of where it points.

### `HostState` and bindgen

Both interface versions are bound, each `bindgen!` in its own module so their
generated types don't collide:

```rust
mod v2 { wasmtime::component::bindgen!({ world: "sync-plugin", path: "sync-plugin.wit"    }); }
mod v1 { wasmtime::component::bindgen!({ world: "sync-plugin", path: "sync-plugin-v1.wit" }); }

struct HostState {
    host_canister_id: Principal,
    callable: CallableCanisters,          // name → principal, from the manifest
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
Both imports use `tokio::runtime::Handle::current().block_on(...)` because the
caller already wraps the synchronous `run_plugin` in
`tokio::task::block_in_place`. For a v0.2.0 plugin the target is resolved from
the request's `call-target` by `resolve_call_target`, which enforces the
`callable` set; for a v0.1.0 plugin the target is always `host_canister_id`.
When a proxy is configured and the call is a non-`direct` update, it is encoded
as `ProxyArgs` and routed through the proxy's `proxy` method; otherwise it goes
straight to the resolved target via `ic-agent`.

### Metadata reads (two routes, one answer)

`get-metadata-section` cannot reuse the call path: `read_state` is not a canister
method, so a proxy canister has nothing to forward. The two routes are therefore
different protocols reaching the same data, chosen by the request's `direct` flag
exactly as `canister-call` chooses one:

- **Direct** — `Agent::read_state_canister_metadata`, signed by the sync
  identity. Absence is *proven* by the certificate, surfacing as
  `AgentError::LookupPathAbsent`, which the host maps to `Ok(None)`.
- **Proxied** — `ProxyArgs` aimed at the management canister's
  `canister_metadata`, so the controller check runs against the proxy. This is
  the same shape the CLI's own management calls take through
  `update_or_proxy_raw`; the runtime inlines it rather than depending on the CLI.

The routes disagree on how absence arrives: the management canister *rejects*
("The canister `<id>` has no metadata section with the name `<name>`.") and the
proxy hands the plugin a reason string with no reject code attached, so the host
matches `NO_SUCH_SECTION_REJECT` against it to produce the same `Ok(None)` a
direct read proves. Matching replica text is the price of one uniform contract;
it fails in the safe direction — a reword upstream turns absence back into an
error rather than into a wrong answer.

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
are checked in; `sync-plugin-v1.wit` is the frozen v0.1.0 contract. Inputs the
v0.1.0 `sync-exec-input` has no field for — `canister-ids` and `fields` — are
simply dropped for a v1 plugin; a v1 plugin cannot observe them, so declaring
`fields:` alongside one has no effect.

### Compute budget (epoch interruption)

The compute-time limit is enforced with wasmtime's epoch interruption: a
background thread calls `Engine::increment_epoch` once per second, and the store
deadline (`set_epoch_deadline`) bounds pure wasm execution. Because a host
call blocks the guest while the host awaits the network, both imports record the
elapsed time (`refund_host_call_time`) and the `epoch_deadline_callback` grants
it back via `epoch_extension` — so network latency is *not* charged against the
limit. The
ticker thread stops when its RAII guard drops at the end of `run_plugin`.

The deadline in seconds is the `compute_limit_secs` parameter. The CLI resolves
it from the `ICP_CLI_PLUGIN_COMPUTE_LIMIT_SECS` environment variable, defaulting
to `DEFAULT_PLUGIN_COMPUTE_LIMIT_SECS` (60) when unset.

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
    pub source: SourceField,              // path: or url:
    pub sha256: Option<String>,
    pub dirs: Option<NamedPaths>,
    pub files: Option<NamedPaths>,
    pub fields: Option<BTreeMap<String, String>>, // inline key-value fields
    pub canisters: Option<Vec<String>>, // extra callable canisters, by name
}
```

`NamedPaths` is an untagged `List(Vec<String>) | Map(IndexMap<String, PathOrList>)`
— the two shapes `dirs:`/`files:` may be written in — keeping the written form
exact, so bundling can rewrite the paths (`map_paths`) and serialize the step
back out unchanged in shape. `entries()` flattens either form to ordered
`(key, path)` pairs: `key` is `None` for a list entry and `Some(name)` for a map
entry, and is *non-unique* — a map key holding a list of paths yields one entry
per path, all sharing the key. The CLI passes those to the runtime as
`KeyedPath`s (this crate stays free of manifest types), which surface in
`sync-exec-input.dirs`/`files` as each entry's `key`.

Each `canisters:` entry is a canister name resolved against the project's ID
table for the environment being synced. `Deserialize` is hand-written to reject a
`url` source without a `sha256`. `fields` is a `BTreeMap` rather than a `HashMap`
so that re-serializing the adapter (the bundler writes a consolidated manifest)
is byte-stable; the WIT interface itself makes no promise about the order fields
arrive in.

Each `fields` value deserializes through `FieldValue`, which takes any YAML
scalar and stringifies it, so `retries: 3` need not be quoted. `serde_yaml` does
that coercion itself when reading YAML *text*, but a canister's build/sync
section reaches the adapter as an already-parsed `serde_yaml::Value` (see
`CanisterManifest`'s hand-written `Deserialize`), and re-deserializing from a
`Value` keeps a number a number — hence the explicit visitor. Lists, mappings,
and empty values are rejected: there is no string to hand the plugin.

### `crates/icp/src/canister/sync/plugin.rs`

Resolves the wasm (local read or remote HTTP fetch into the package cache),
verifies sha256, builds the exposed canister ID table and the `CallableCanisters`
enforcement set (resolving `canisters:` against the project's IDs), then calls
`icp_sync_plugin::run_plugin(...)` with a `PluginInvocation`. The runtime — not
the CLI — opens the declared paths and enforces the path-safety checks, so the
CLI no longer touches the plugin's input files itself; it supplies the canister
directory and the project directory (`sync::Params::path` and `project_dir`)
that bound them. `exposed_canister_ids`
adds a bare-local-name duplicate for every canister in the same subproject as
the one being synced; `resolve_callable` fails the step if a name in
`canisters:` does not resolve.
