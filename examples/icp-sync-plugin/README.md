# icp-sync-plugin example

This example demonstrates the sync plugin system: a Wasm component that runs
inside `icp sync` and drives canister update calls on behalf of the user.

## Project structure

```
icp-sync-plugin/
├── canister/          # The target canister (compiled to wasm32-unknown-unknown)
├── plugin/            # The sync plugin (compiled to wasm32-wasip2)
├── seed-data/         # Fruit files the plugin registers (preopened via WASI)
└── icp.yaml           # Manifest wiring the build and sync steps together
```

### `canister/`

A simple Rust canister with three methods:

| Method | Type | Description |
|---|---|---|
| `set_uploader(principal)` | update | Stores a principal as the authorised uploader. Restricted to canister controllers. |
| `register(name, content)` | update | Appends a `(name, content)` fruit pair. Restricted to the stored uploader. |
| `show()` | query | Returns the current uploader principal and all registered fruits. |

### `plugin/`

A Rust Wasm component that implements the `sync-plugin` world defined in
`crates/icp-sync-plugin/sync-plugin.wit`. The host runtime calls its `exec`
export and provides a `canister-call` import the plugin uses to reach the
canister.

## How the plugin system is exercised

This example is designed to demonstrate both routing modes of the
`canister-call` import — the `direct` flag — in a single sync run.

### Call 1 — `set_uploader` via proxy (`direct: false`)

The plugin reads `identity-principal` from `sync-exec-input` (the signing
identity the CLI is using) and calls `set_uploader` with it. The call is
routed through the proxy canister (`direct: false`), so it arrives at the
target canister with the **proxy's principal as the caller**. Because the proxy
canister is listed as a controller of the target, the controller guard passes.

This models a pattern where privileged, one-time setup calls must come from a
known controller — not directly from an end-user identity.

### Call 2 — `register` directly (`direct: true`)

For each file under `seed-data/`, the plugin calls `register` with
`direct: true`, bypassing the proxy entirely. The call arrives at the canister
with the **user's identity principal as the caller**, which is exactly the
uploader stored in step 1, so the uploader guard passes.

This models a pattern where bulk data-upload calls must be attributable to the
actual user identity rather than a shared proxy.

### Data flow summary

```
icp sync
  └─ host runtime loads plugin.wasm
       ├─ exec(sync-exec-input) called
       │    identity-principal = <user>
       │    proxy-canister-id  = <proxy>
       │
       ├─ canister-call set_uploader(<user>)   direct=false → proxy → canister
       │    canister stores uploader = <user>
       │
       └─ canister-call register(name, content) direct=true  → canister  (× N files)
            canister checks caller == uploader  ✓
```

## Building

```bash
# Build the canister
cargo build --target wasm32-unknown-unknown --release -p canister

# Build the plugin
cargo build --target wasm32-wasip2 --release -p plugin
```
