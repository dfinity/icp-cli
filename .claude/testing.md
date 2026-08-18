# Testing

## Test Structure

Tests are split between unit tests (in modules) and integration tests:

- Integration tests in `crates/icp-cli/tests/` test full command execution
- Use `assert_cmd` for CLI assertions and `predicates` for output matching
- Use `serial_test` with file locks for tests that share resources (network ports)
- Some tests launch local networks and require available ports
- Only one process-global `tracing` subscriber may be installed per test binary. In `icp-cli`'s
  unit tests that is `events.rs::tests::captured_debug_lines`, which captures `debug!` lines;
  installing a second one panics it. It has to be global rather than thread-local because
  `tracing` decides whether a callsite is enabled the first time any thread reaches it and
  caches that answer for the process.

## Mock Helpers

`crates/icp/src/lib.rs` provides test utilities:

- `MockProjectLoader::minimal()`: Single canister, network, environment
- `MockProjectLoader::complex()`: Multiple canisters, networks, environments
- `NoProjectLoader`: Simulates missing project for error cases
