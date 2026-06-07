---
title: Sync Plugins
description: How sync plugins extend the sync phase with sandboxed WebAssembly components that run arbitrary post-deployment logic against a canister.
---

A **sync plugin** is a WebAssembly component that runs during the [sync phase](build-deploy-sync.md#sync-phase) to perform arbitrary post-deployment work against a single canister. icp-cli loads the plugin into a sandboxed [wasmtime](https://wasmtime.dev/) WASI runtime, hands it the ID of the canister being synced, and lets it make canister calls and read declared files — nothing more.

You declare a sync plugin in your manifest with a `plugin` sync step. For the exact manifest fields, see [Plugin Sync in the Configuration Reference](../reference/configuration.md#plugin-sync). To author your own plugin, see [Writing a Sync Plugin](../guides/writing-sync-plugins.md).

## Why Sync Plugins

The built-in [`script` sync step](build-deploy-sync.md#script-sync-steps) covers simple post-deployment commands, but shelling out has drawbacks: scripts are unstructured, run with your full user privileges, and can't be distributed as a single verifiable artifact.

Sync plugins fill that gap. A plugin is:

- **Portable** — written in any language that compiles to `wasm32-wasip2`, distributed as one `.wasm` file (local path or remote URL + `sha256`).
- **Sandboxed** — it cannot open network sockets, spawn subprocesses, or touch the filesystem outside the directories you explicitly grant it.
- **Scoped to one canister** — it can call update and query methods, but only on the canister being synced. The target is fixed by the host; the plugin cannot choose a different one.

The most common way to get a sync plugin is through a [recipe](recipes.md). For example, the `@dfinity/asset-canister` recipe emits a `plugin` sync step (starting with `v2.2.1`) that uploads your built static files to the asset canister — so for everyday frontend deployment you never write a plugin yourself.

## How a Plugin Runs

When a `plugin` sync step executes for a canister, icp-cli:

1. Resolves the wasm — reads the local `path`, or downloads the `url` to the package cache.
2. Verifies the `sha256` checksum if one is given (required for `url`).
3. Reads any files listed in `files:` and preopens any directories listed in `dirs:` read-only.
4. Instantiates the component in a WASI sandbox and calls its `exec()` export.
5. Forwards the plugin's output to the CLI and reports success or the returned error.

```
icp sync
  └─ host loads plugin.wasm into the WASI sandbox
       ├─ exec(sync-exec-input) called
       │    canister-id        = <canister being synced>
       │    identity-principal = <your signing identity>
       │    dirs / files       = what you declared in the manifest
       │
       └─ plugin makes canister-call(...) to the target canister (× N)
```

## The Plugin Interface

The interface is defined as a [WIT](https://component-model.bytecodealliance.org/design/wit.html) world. The host provides one import (`canister-call`); the plugin provides one export (`exec`):

```wit
world sync-plugin {
    // Host import: call the canister being synced.
    import canister-call: func(req: canister-call-request) -> result<list<u8>, string>;

    // Plugin export: run the sync step.
    export exec: func(input: sync-exec-input) -> result<_, string>;
}
```

The authoritative interface, including all record fields, lives in [`sync-plugin.wit`](https://github.com/dfinity/icp-cli/blob/main/crates/icp-sync-plugin/sync-plugin.wit) in the icp-cli repository.

### What the plugin receives — `sync-exec-input`

| Field | Description |
|-------|-------------|
| `canister-id` | Textual principal of the canister being synced |
| `environment` | Name of the environment being synced (e.g. `local`, `production`) |
| `dirs` | The directories you declared in `dirs:`; the host preopened each one read-only |
| `files` | The files you declared in `files:`, each as a `(name, content)` pair read by the host |
| `identity-principal` | Textual principal of the signing identity used for canister calls |
| `proxy-canister-id` | Textual principal of the proxy canister if one was configured via `--proxy`, otherwise absent |

### Calling the canister — `canister-call`

The plugin calls methods on the target canister through the `canister-call` import. It supplies the method name, **Candid-encoded argument bytes** (the host forwards them unchanged), and a few routing options:

| Request field | Meaning |
|---------------|---------|
| `method` | The canister method to call |
| `arg` | Candid-encoded argument bytes (the plugin encodes; the host forwards as-is) |
| `call-type` | `update` or `query` |
| `direct` | When `false` (default), update calls are routed through the [proxy canister](../guides/proxy-canister.md) if one is configured; when `true`, the call always goes directly to the target. Query calls always go directly regardless. |
| `cycles` | Cycles to attach to a proxied update call; only meaningful when `direct` is `false`, a proxy is configured, and `call-type` is `update` |

The host always calls the canister named in `sync-exec-input.canister-id`. There is no field for a different canister ID — the single-canister restriction is structural, not a policy the plugin can opt out of.

### Logging — stdout and stderr

The plugin's stdout and stderr are captured by the host (no logging import is needed — use ordinary `println!` / `eprintln!`):

- **stdout** is shown as transient progress in the rolling step view and discarded when the step ends. Use it for in-flight chatter.
- **stderr** is shown in the rolling view **and** printed persistently after the step completes successfully. Use it for messages the user must still see afterward — warnings, summaries, deprecation notices.

Each stream is capped at 1 MiB; output beyond that is truncated with a note.

## The Sandbox

The plugin runs with a deliberately narrow capability surface.

### Filesystem

- Each directory in `dirs:` is preopened **read-only**. The plugin sees it at the same relative path it used in the manifest (e.g. `dirs: ["assets"]` is visible as `assets/` inside the guest) and traverses it with standard filesystem APIs (`std::fs` in Rust).
- Files in `files:` are read by the host up front and passed inline in `sync-exec-input.files`. The plugin reads their content from the input struct, not from disk.
- Any path outside a preopen is invisible. Writes, creates, deletes, renames, and symlinks that escape a preopen are rejected. Paths in `dirs:`/`files:` must be relative and may not contain `..`.

### Capabilities

| Capability | Available? | Notes |
|------------|------------|-------|
| Read declared `dirs:` | yes | read-only preopens |
| Clocks, RNG, `wasi:io` | yes | Rust's `HashMap`, `chrono`, etc. work normally |
| `process::exit` / panics | yes | abort the guest cleanly; the host surfaces the error |
| Canister calls | yes | only to the canister being synced |
| Environment variables / args | no | the WASI environment is empty; use `sync-exec-input.environment` |
| Network sockets / DNS | blocked | treat the network as unavailable |
| Filesystem writes | blocked | no writable preopens |
| Spawning subprocesses | blocked | no process interface is linked |

### Resource limits

| Resource | Limit |
|----------|-------|
| Wasm call-stack depth | 512 KiB |
| Pure compute time | 60 seconds |
| Linear memory | wasm32 address space (≤ 4 GiB) |
| stdout / stderr per stream | 1 MiB |

The 60-second budget counts only wasm instruction execution. Time spent waiting for a `canister-call` to return over the network is **not** charged against it — the host grants that time back when the call completes. A plugin can make as many canister calls as it needs without the network latency eating into its compute limit.

## Next Steps

- [Writing a Sync Plugin](../guides/writing-sync-plugins.md) — Author your own plugin in Rust
- [Plugin Sync (Configuration Reference)](../reference/configuration.md#plugin-sync) — The manifest fields
- [Build, Deploy, Sync](build-deploy-sync.md) — Where the sync phase fits in the lifecycle
- [Recipes](recipes.md) — How recipes can emit a `plugin` sync step for you

[Browse all documentation →](../index.md)
