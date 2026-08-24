---
title: Sync Plugins
description: How sync plugins extend the sync phase with sandboxed WebAssembly components that run arbitrary post-deployment logic against a canister.
---

A **sync plugin** is a WebAssembly component that runs during the [sync phase](build-deploy-sync.md#sync-phase) to perform arbitrary post-deployment work. icp-cli loads the plugin into a sandboxed [wasmtime](https://wasmtime.dev/) WASI runtime, hands it the ID of the canister being synced (plus the project's canister ID table), and lets it make canister calls, read canister metadata, and read declared files — nothing more. By default it can call only the canister being synced; it may call other canisters it lists in the sync step's `canisters:` list.

You declare a sync plugin in your manifest with a `plugin` sync step. For the exact manifest fields, see [Plugin Sync in the Configuration Reference](../reference/configuration.md#plugin-sync). To author your own plugin, see [Writing a Sync Plugin](../guides/writing-sync-plugins.md).

## Why Sync Plugins

The built-in [`script` sync step](build-deploy-sync.md#script-sync-steps) covers simple post-deployment commands, but shelling out has drawbacks: scripts are unstructured, run with your full user privileges, and can't be distributed as a single verifiable artifact.

Sync plugins fill that gap. A plugin is:

- **Portable** — written in any language that compiles to `wasm32-wasip2`, distributed as one `.wasm` file (local path or remote URL + `sha256`).
- **Sandboxed** — it cannot open network sockets, spawn subprocesses, or touch the filesystem outside the directories you explicitly grant it.
- **Scoped by declaration** — it can call update and query methods on the canister being synced, plus any canister listed in the manifest's `canisters:` list. A call to a canister that was not listed is rejected by the host.

The most common way to get a sync plugin is through a [recipe](recipes.md). For example, the `@dfinity/asset-canister` recipe emits a `plugin` sync step (starting with `v2.2.1`) that uploads your built static files to the asset canister — so for everyday frontend deployment you never write a plugin yourself.

## How a Plugin Runs

When a `plugin` sync step executes for a canister, icp-cli:

1. Resolves the wasm — reads the local `path`, or downloads the `url` to the package cache.
2. Verifies the `sha256` checksum if one is given (required for `url`).
3. Reads any files listed in `files:`, preopens any directories listed in `dirs:` read-only, and collects any key-value pairs listed in `fields:`.
4. Instantiates the component in a WASI sandbox and calls its `exec()` export.
5. Forwards the plugin's output to the CLI and reports success or the returned error.

```
icp sync
  └─ host loads plugin.wasm into the WASI sandbox
       ├─ exec(sync-exec-input) called
       │    canister-id        = <canister being synced>
       │    identity-principal = <your signing identity>
       │    canister-ids       = <name → principal table for the environment>
       │    dirs/files/fields  = what you declared in the manifest
       │
       └─ plugin makes canister-call({ target, ... }) (× N)
          and canister-metadata-section({ target, name })
            target = host (the canister being synced), or a
                     canister from `canisters:` by name
```

## The Plugin Interface

The interface is defined as a [WIT](https://component-model.bytecodealliance.org/design/wit.html) world. The host provides two imports (`canister-call` and `canister-metadata-section`); the plugin provides one export (`exec`):

```wit
world sync-plugin {
    // Host import: call the canister being synced or one listed in `canisters:`.
    import canister-call: func(req: canister-call-request) -> result<list<u8>, string>;

    // Host import: read a metadata section from one of those same canisters.
    import canister-metadata-section: func(req: metadata-section-request) -> result<option<list<u8>>, string>;

    // Plugin export: run the sync step.
    export exec: func(input: sync-exec-input) -> result<_, string>;
}
```

The interface is versioned (currently `icp:sync-plugin@0.2.0`). icp-cli reads the version a plugin was built against from the component itself and drives it accordingly, so plugins built against the earlier `@0.1.0` interface — which could only call the canister being synced — continue to load unchanged.

The authoritative interface, including all record fields, lives in [`sync-plugin.wit`](https://github.com/dfinity/icp-cli/blob/main/crates/icp-sync-plugin/sync-plugin.wit) in the icp-cli repository.

### What the plugin receives — `sync-exec-input`

| Field | Description |
|-------|-------------|
| `canister-id` | Textual principal of the canister being synced |
| `environment` | Name of the environment being synced (e.g. `local`, `production`) |
| `dirs` | The directories you declared in `dirs:`; the host preopened each one read-only. Each entry carries its `key` (see below) and `path` |
| `files` | The files you declared in `files:`, each with its `key`, `name` (path), and `content` read by the host |
| `fields` | The key-value fields you declared in `fields:`, each as a `(name, value)` pair; values are strings |
| `identity-principal` | Textual principal of the signing identity used for canister calls |
| `proxy-canister-id` | Textual principal of the proxy canister if one was configured via `--proxy`, otherwise absent |
| `canister-ids` | The project's canister ID table for this environment — each entry a canister name and the principal it resolves to. Informational; being listed here does not grant permission to call a canister |

Each `canister-ids` entry's name is the canister's fully-qualified project key: a bare local name for a canister defined in the app root, or a `subproject:canister` key for a canister defined in a subproject. Canisters in the same subproject as the one being synced are additionally listed under their bare local name, so a plugin can look up a sibling by the name that subproject's manifest uses. A bare name always means the sibling: if an app-root canister has the same local name, it is not listed for that sync.

`dirs` and `files` each carry a `key`: the map key the entry was declared under in the manifest, or absent when `dirs:`/`files:` was written as a plain list. A key that maps to a list of paths produces several entries sharing that key, so the key is not unique. Use it to group or label declared paths — e.g. distinguish `seed:` directories from `migrations:` directories — without hardcoding paths in the plugin.

### Calling a canister — `canister-call`

The plugin calls methods through the `canister-call` import. It picks a `target`, supplies the method name, **Candid-encoded argument bytes** (the host forwards them unchanged), and a few routing options:

| Request field | Meaning |
|---------------|---------|
| `target` | Which canister to call: `host` (the canister being synced), or a canister declared in `canisters:` addressed by `name` |
| `method` | The canister method to call |
| `arg` | Candid-encoded argument bytes (the plugin encodes; the host forwards as-is) |
| `call-type` | `update` or `query` |
| `direct` | When `false` (default), update calls are routed through the [proxy canister](../guides/proxy-canister.md) if one is configured; when `true`, the call always goes directly to the target. Query calls always go directly regardless. |
| `cycles` | Cycles to attach to a proxied update call; only meaningful when `direct` is `false`, a proxy is configured, and `call-type` is `update` |

The `host` target always resolves to `sync-exec-input.canister-id` and is always permitted. A `name` target is permitted only if that canister appears in the sync step's [`canisters:`](../reference/configuration.md#plugin-sync) list; the host rejects any other target without making a call. A name is the only way to address another canister — the host owns the name→principal mapping, which differs per environment.

### Reading canister metadata — `canister-metadata-section`

The plugin reads a canister's [metadata sections](../reference/cli.md#icp-canister-metadata) — `candid:service`, for instance — through the `canister-metadata-section` import:

| Request field | Meaning |
|---------------|---------|
| `target` | Which canister to read from: `host`, or a canister declared in `canisters:` addressed by `name` — the same targets, and the same enforcement, as `canister-call` |
| `name` | The section name, without the `icp:public `/`icp:private ` prefix the wasm custom section carries (e.g. `candid:service`) |
| `direct` | When `false` (default), the read is routed through the [proxy canister](../guides/proxy-canister.md) if one is configured; when `true`, it always goes straight to the target |

A successful read returns the section's raw bytes, or **absent** when the target reports it has no section by that name — so a plugin can probe for an optional section without matching on error text. Anything else (an unreachable canister, a section the caller may not read) is an error.

The two routes differ in who the target sees asking, which decides what a **private** section will yield:

- **Direct** — a certified `read_state` request signed by the sync identity. A private section requires that identity to control the target.
- **Proxied** — a call to the management canister's `canister_metadata` method made by the proxy, because `read_state` is not a canister method and cannot be forwarded. A private section requires the *proxy* to control the target — the same arrangement proxied update calls rely on.

With no proxy configured, both settings read directly.

### Logging — stdout and stderr

The plugin's stdout and stderr are captured by the host (no logging import is needed — use ordinary `println!` / `eprintln!`):

- **stdout** is shown as transient progress in the rolling step view and discarded when the step ends. Use it for in-flight chatter.
- **stderr** is shown in the rolling view **and** printed persistently after the step completes successfully. Use it for messages the user must still see afterward — warnings, summaries, deprecation notices.

Each stream is capped at 1 MiB; output beyond that is truncated with a note.

## The Sandbox

The plugin runs with a deliberately narrow capability surface.

### Filesystem

- Each directory in `dirs:` is readable **read-only**. The plugin sees it at the same relative path it used in the manifest (e.g. `dirs: ["assets"]` is visible as `assets/` inside the guest) and traverses it with standard filesystem APIs (`std::fs` in Rust).
- Entries may name the same directory under several keys, or name a directory inside another entry's, and the plugin is told about each entry as written. The preopens behind them are one per distinct tree: an entry nested inside another is read through the preopen covering it, which grants nothing extra.
- Files in `files:` are read by the host up front and passed inline in `sync-exec-input.files`. The plugin reads their content from the input struct, not from disk.
- Any path outside a preopen is invisible. Writes, creates, deletes, renames, and symlinks that escape a preopen are rejected by the sandbox at runtime.
- Paths in `dirs:`/`files:` must be relative and may not contain `..`. They also may not be — or traverse — a symlink: each declared entry is rejected if it or any of its parent components is a symlink, so a declared path cannot resolve to a target outside the canister directory. (This restriction may be relaxed later if a safe use case emerges.)

### Capabilities

| Capability | Available? | Notes |
|------------|------------|-------|
| Read declared `dirs:` | yes | read-only preopens |
| Clocks, RNG, `wasi:io` | yes | Rust's `HashMap`, `chrono`, etc. work normally |
| `process::exit` / panics | yes | abort the guest cleanly; the host surfaces the error |
| Canister calls | yes | to the canister being synced, and to canisters declared in `canisters:` |
| Canister metadata reads | yes | the same set of canisters as calls |
| Environment variables / args | no | the WASI environment is empty; use `sync-exec-input.environment` |
| Network sockets / DNS | blocked | treat the network as unavailable |
| Filesystem writes | blocked | no writable preopens |
| Spawning subprocesses | blocked | no process interface is linked |

### Resource limits

| Resource | Limit |
|----------|-------|
| Wasm call-stack depth | 512 KiB |
| Pure compute time | 60 seconds (default) |
| Linear memory | wasm32 address space (≤ 4 GiB) |
| stdout / stderr per stream | 1 MiB |

The compute-time budget defaults to 60 seconds and is overridable with the [`ICP_CLI_PLUGIN_COMPUTE_LIMIT_SECS`](../reference/environment-variables.md#icp_cli_plugin_compute_limit_secs) environment variable — raise it for compute-heavy plugins (e.g. compressing a large asset bundle) that legitimately need more time, especially on slower CI runners. The budget counts only wasm instruction execution: time spent waiting for a host call (`canister-call`, `canister-metadata-section`) to return over the network is **not** charged against it — the host grants that time back when the call completes. A plugin can make as many canister calls as it needs without the network latency eating into its compute limit.

## Next Steps

- [Writing a Sync Plugin](../guides/writing-sync-plugins.md) — Author your own plugin in Rust
- [Plugin Sync (Configuration Reference)](../reference/configuration.md#plugin-sync) — The manifest fields
- [Build, Deploy, Sync](build-deploy-sync.md) — Where the sync phase fits in the lifecycle
- [Recipes](recipes.md) — How recipes can emit a `plugin` sync step for you

[Browse all documentation →](../index.md)
