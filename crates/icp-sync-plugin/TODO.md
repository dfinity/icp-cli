# Sync Plugin TODO

## Wasm caching

Cache remote plugin wasm files in `.icp/cache/` so they are not re-downloaded
on every sync. Key the cache entry on the sha256 checksum. When a remote step
has a sha256 and the cached file matches, skip the HTTP fetch entirely.

## Plugin timeout

Add `timeout_seconds: Option<u64>` to `manifest::adapter::plugin::Adapter`
and wire it through `sync/plugin.rs` → `run_plugin`. Use wasmtime's
epoch-based interruption (`Engine::increment_epoch` on a background thread,
`Store::set_epoch_deadline`) to interrupt a plugin that runs too long.

## Integration tests

Add end-to-end tests that compile the `examples/icp-sync-plugin/plugin`
against a mock canister and verify the full `run_plugin` path (wasm load,
WASI preopen, canister-call, stdio capture). Unit-level tests for sha256
mismatch and the remote-download path in `sync/plugin.rs` would also improve
coverage.
