# Testing

## Test Structure

Tests are split between unit tests (in modules) and integration tests:

- Integration tests in `crates/icp-cli/tests/` test full command execution
- Use `assert_cmd` for CLI assertions and `predicates` for output matching
- Use `serial_test` with file locks for tests that share resources (network ports)
- Some tests launch local networks and require available ports

## Mock Helpers

`crates/icp/src/lib.rs` provides test utilities:

- `MockProjectLoader::minimal()`: Single canister, network, environment
- `MockProjectLoader::complex()`: Multiple canisters, networks, environments
- `NoProjectLoader`: Simulates missing project for error cases
