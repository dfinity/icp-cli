# Sync Plugin Sandbox

Sync plugins are untrusted WebAssembly components. `icp-cli` runs them inside
a [wasmtime](https://wasmtime.dev/) Component Model sandbox with a deliberately
narrow capability surface. This document describes exactly what a plugin can
and cannot do at runtime.

## Host interface

The plugin's only guaranteed way to interact with the outside world is through
the imports declared in [`sync-plugin.wit`](sync-plugin.wit):

- `canister-call` — update or query call against the target canister only.
  The plugin does **not** choose the target; the host fixes it to the
  canister being synced.

That's it. The plugin cannot call other canisters, switch identities, or
reach the management canister.

## Filesystem

- The host preopens each directory listed in the manifest's `dirs:` field
  **read-only** (`DirPerms::READ`, `FilePerms::READ`).
- The plugin sees each preopen at the same relative path it used in the
  manifest (e.g. `dirs: ["assets"]` is visible as `assets/` inside the guest).
- Files listed in `files:` are read by the host and passed inline in
  `sync-exec-input.files`; the plugin never opens them itself.
- Any path not covered by a preopen is invisible. Writes, creates, deletes,
  renames, and symlinks that escape a preopen are rejected by wasmtime.

If your plugin needs to emit files (generated code, caches), do it through
the canister or request the feature — writable preopens are not currently
supported.

## WASI capabilities

The host links the standard `wasi:cli/imports` world. In practice only a
subset is usable because the default `WasiCtx` denies the rest:

**Available:**

- `wasi:filesystem` — constrained to the read-only preopens described above.
- `wasi:io`, `wasi:clocks` (wall + monotonic), `wasi:random` — timestamps,
  RNG, stream I/O. Safe to rely on (Rust's `HashMap`, `chrono`, `log`, etc.
  work normally).
- `wasi:cli/exit` — `process::exit` and panics abort the guest instance
  cleanly; the host reports the error and continues.
- `wasi:cli/environment` — returns **empty** env and args. Do not depend on
  environment variables; use `sync-exec-input.environment` instead.
- `wasi:cli/terminal-*` — reports "not a terminal". Libraries that
  auto-detect color will simply disable it.

**Linked but effectively blocked:**

- `wasi:sockets` (TCP, UDP, DNS) — all addresses are denied by default, so
  `connect`, `bind`, and name lookups fail. Treat network as unavailable.
  Plugins that need external data should fetch it via the canister.

**Stdio:**

- `stdin` is closed.
- `stdout` and `stderr` are captured by the host. After `exec()` returns,
  stdout is forwarded to the CLI's progress output first, then stderr.
  Invalid UTF-8 is replaced with U+FFFD.
- Use your language's normal print facilities (e.g. Rust's `println!` /
  `eprintln!`, or any `log` / `tracing` backend that writes to stderr).
  There is no separate host `log` import.

## What this means for plugin authors

You can:

- Read any file under a declared `dirs:` entry.
- Use standard language features that rely on clocks, RNG, or filesystem
  reads.
- Panic or exit — the host will surface the error.

You cannot:

- Open network connections or resolve DNS.
- Write to disk, spawn subprocesses, or read environment variables.
- Call canisters other than the one being synced.
- Escape a preopen via `..` or symlinks.

## What this means for users

A sync plugin is confined to reading the directories and files its manifest
step declares, plus talking to the single canister that step targets. It
cannot exfiltrate data over the network, touch files outside the declared
paths, or interact with other canisters on your behalf. Review the `dirs:`
and `files:` lists in your manifest — those define the plugin's entire view
of your project.
