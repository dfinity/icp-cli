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

`wit_bindgen::generate!` reads the WIT at build time and produces the `Guest` trait you implement, the input/request types, and the `canister_call` host function. The `exec` export is your entry point — it returns `Ok(())` on success or `Err(message)` to fail the sync step.

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
- **The target is fixed.** `canister_call` always reaches the canister in `input.canister_id` — there is no field to target another canister.
- **`direct` and `cycles` control proxy routing.** With `direct: false`, update calls go through the [proxy canister](proxy-canister.md) when one is configured, and `cycles` can fund the forwarded call. With `direct: true`, the call always goes straight to the target. See [The Plugin Interface](../concepts/sync-plugins.md#the-plugin-interface) for the full semantics.

## Read Declared Files and Directories

A plugin can't see the filesystem freely — only what you grant it in the manifest's `dirs:` and `files:`.

Directories in `dirs:` are preopened read-only at the same relative path. Traverse them with standard `std::fs`:

```rust
for dir in &input.dirs {
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        // ... encode and send to the canister ...
    }
}
```

Files in `files:` are read by the host up front and passed inline — read them from the input struct, not from disk:

```rust
for file in &input.files {
    println!("{} = {}", file.name, file.content.trim());
}
```

Writes, paths outside a preopen, and `..` traversal are all rejected by the sandbox. See [The Sandbox](../concepts/sync-plugins.md#the-sandbox) for the full capability list and resource limits.

## Build

```bash
cargo build --target wasm32-wasip2 --release
```

The output `.wasm` (under `target/wasm32-wasip2/release/`) is loaded directly by icp-cli — no extra component-packaging step is required.

## Wire It Into the Manifest

Reference the built wasm from a `plugin` sync step and declare the files and directories the plugin needs:

```yaml
sync:
  steps:
    - type: plugin
      path: target/wasm32-wasip2/release/my_plugin.wasm
      dirs:
        - seed-data
      files:
        - config.txt
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
