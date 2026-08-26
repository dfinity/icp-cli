---
title: Writing a Sync Plugin
description: Author a WebAssembly sync plugin in Rust that runs sandboxed post-deployment logic against a canister during icp sync.
---

This guide walks through writing a [sync plugin](../concepts/sync-plugins.md) in Rust — a WebAssembly component that icp-cli runs during `icp sync` to perform post-deployment work against a canister. If you only want to *use* an existing plugin (for example, one emitted by a recipe), you don't need this guide; see [Plugin Sync in the Configuration Reference](../reference/configuration.md#plugin-sync) instead.

For a complete, runnable project, see the [`icp-sync-plugin` example](https://github.com/dfinity/icp-cli/tree/main/examples/icp-sync-plugin).

## Prerequisites

A plugin compiles to the `wasm32-wasip2` target. Add it once:

```bash
rustup target add wasm32-wasip2
```

You also need the plugin interface definition, [`sync-plugin.wit`](https://github.com/dfinity/icp-cli/blob/main/crates/icp-sync-plugin/sync-plugin.wit). Copy it into your plugin crate (e.g. as `sync-plugin.wit`) so the build can generate bindings from it. The `.wit` file is the source of truth for the interface.

## Set Up the Crate

A plugin is a `cdylib` crate. Its `Cargo.toml` needs `candid` (to encode call arguments) and `wit-bindgen` (to generate the interface bindings):

```toml
[package]
name = "my-plugin"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
candid = "0.10"
wit-bindgen = { version = "0.56", features = ["realloc"] }
```

## Generate Bindings and Implement `exec`

`wit_bindgen::generate!` reads the WIT at build time and produces the `Guest` trait you implement, the input/request types, and the host functions (`canister_call`, `canister_metadata_section`). The `exec` export is your entry point — it returns `Ok(())` on success or `Err(message)` to fail the sync step.

```rust
// src/lib.rs
wit_bindgen::generate!({
    world: "sync-plugin",
    path: "sync-plugin.wit",
});

use candid::{Encode, Principal};

struct Plugin;

impl Guest for Plugin {
    fn exec(input: SyncExecInput) -> Result<(), String> {
        // stdout: transient progress, discarded when the step ends.
        println!(
            "syncing canister {} (environment: {})",
            input.canister_id, input.environment
        );

        // Encode the Candid argument yourself; the host forwards the bytes unchanged.
        let uploader = Principal::from_text(&input.identity_principal)
            .map_err(|e| format!("invalid identity principal: {e}"))?;
        let arg = Encode!(&uploader).map_err(|e| format!("encode arg: {e}"))?;

        // Call a method on the canister being synced.
        canister_call(&CanisterCallRequest {
            target: CallTarget::Host, // the canister being synced
            method: "set_uploader".to_string(),
            arg,
            call_type: icp::sync_plugin::types::CallType::Update,
            direct: false, // route update calls through the proxy if one is configured
            cycles: 0,
        })?;

        // stderr: printed persistently after the step completes — use for summaries.
        eprintln!("set_uploader: ok");
        Ok(())
    }
}

export!(Plugin);
```

A few things to note:

- **You encode the arguments.** `arg` is raw Candid bytes. Encode with `candid::Encode!`; decode any response (`Vec<u8>`) with `candid::Decode!`.
- **You choose the target.** `target: CallTarget::Host` reaches the canister being synced. To call another canister, declare it in the manifest's [`canisters:`](../reference/configuration.md#plugin-sync) list and address it with `CallTarget::Name("ledger".into())` — the name matches the entries in `input.canister_ids`. Names are the only way to reach another canister; the host resolves them per environment. The host rejects a target you did not declare. A name is always the one the plugin's own project uses, so hardcoding it stays correct when that project is vendored into a workspace as a subproject.
- **`direct` and `cycles` control proxy routing.** With `direct: false`, update calls go through the [proxy canister](proxy-canister.md) when one is configured, and `cycles` can fund the forwarded call. With `direct: true`, the call always goes straight to the target. See [The Plugin Interface](../concepts/sync-plugins.md#the-plugin-interface) for the full semantics.

## Read Canister Metadata

`canister_metadata_section` reads a [metadata section](../reference/cli.md#icp-canister-metadata) off a canister — useful for inspecting what is actually deployed before acting on it, e.g. its `candid:service` interface:

```rust
let interface = canister_metadata_section(&MetadataSectionRequest {
    target: CallTarget::Host, // same targets, same rules, as canister_call
    name: "candid:service".to_string(),
    direct: false, // route through the proxy if one is configured
})?;

match interface {
    Some(bytes) => println!("interface: {}", String::from_utf8_lossy(&bytes)),
    // `None` means the canister provably has no such section — not a failure.
    // A section you may not read, or a canister that does not exist, is an error.
    None => println!("canister exposes no Candid interface"),
}
```

`direct` picks who the target sees asking, which is what decides whether a *private* section is readable: a direct read is signed by the sync identity, a proxied one is made by the proxy canister on your behalf. See [Reading canister metadata](../concepts/sync-plugins.md#reading-canister-metadata--canister-metadata-section) for the full semantics.

## Read Declared Files and Directories

A plugin can't see the filesystem freely — only what you grant it in the manifest's `dirs:` and `files:`.

Directories in `dirs:` are preopened read-only at the same relative path. Each entry gives you its `path` plus a `key` (the map key it was declared under, or `None` for a plain-list entry). Traverse them with standard `std::fs`:

```rust
for dir in &input.dirs {
    for entry in std::fs::read_dir(&dir.path).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        // ... encode and send to the canister; dir.key groups related dirs ...
    }
}
```

Files in `files:` are read by the host up front and passed inline — read them from the input struct, not from disk. Each entry carries its `key`, `name` (the path), and `content`:

```rust
for file in &input.files {
    println!("{} = {}", file.name, file.content.trim());
}
```

Declaring `dirs:`/`files:` as a map instead of a list tags each entry with a `key`, so a plugin can group or label paths (for example, tell `seed:` directories from `migrations:`) without hardcoding paths. A key that maps to a list of paths yields several entries sharing that key.

Open each entry at the `path` it arrives with, whatever it looks like: a manifest may declare a directory elsewhere in the project (`../shared/assets`), and the preopen carries that same spelling. Writes, and paths that escape a preopen, are rejected by the sandbox at runtime. See [The Sandbox](../concepts/sync-plugins.md#the-sandbox) for the full capability list and resource limits.

## Read Declared Fields

Key-value pairs declared in the manifest's `fields:` are passed inline as string values. Use them for small configuration a plugin needs without shipping a file:

```rust
for field in &input.fields {
    println!("{} = {}", field.name, field.value);
}
```

A value always arrives as a string, so parse the ones you want as another type — a manifest may write `retries: 3` unquoted, and the plugin receives `"3"`.

## Build

```bash
cargo build --target wasm32-wasip2 --release
```

The output `.wasm` (under `target/wasm32-wasip2/release/`) is loaded directly by icp-cli — no extra component-packaging step is required.

## Wire It Into the Manifest

Reference the built wasm from a `plugin` sync step and declare the files, directories, and fields the plugin needs:

```yaml
sync:
  steps:
    - type: plugin
      path: target/wasm32-wasip2/release/my_plugin.wasm
      dirs:
        - seed-data
      files:
        - config.txt
      fields:
        api_url: https://example.com
        retries: 3
```

Then run the sync phase:

```bash
icp sync my-canister
```

For remote distribution, host the `.wasm` and reference it with `url` plus a required `sha256`. See [Plugin Sync](../reference/configuration.md#plugin-sync) for all manifest fields.

## Next Steps

- [Sync Plugins](../concepts/sync-plugins.md) — The mechanism, interface, and sandbox in depth
- [Plugin Sync (Configuration Reference)](../reference/configuration.md#plugin-sync) — The manifest fields
- [Proxy Canister](proxy-canister.md) — How proxied update calls and cycles work
- [`icp-sync-plugin` example](https://github.com/dfinity/icp-cli/tree/main/examples/icp-sync-plugin) — A complete working project
