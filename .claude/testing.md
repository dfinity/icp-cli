# Testing

## Test Structure

Tests are split between unit tests (in modules) and integration tests:

- Integration tests in `crates/icp-cli/tests/` test full command execution
- Use `assert_cmd` for CLI assertions and `predicates` for output matching
- Use `serial_test` with file locks for tests that share resources (network ports)
- Some tests launch local networks and require available ports

## Mock Helpers

`crates/icp-project/src/lib.rs` provides test utilities:

- `MockProjectLoader::minimal()`: Single canister, network, environment
- `MockProjectLoader::complex()`: Multiple canisters, networks, environments
- `NoProjectLoader`: Simulates missing project for error cases

These, and the mock for every seam in `icp_project::host::Host`, sit behind
`icp-project`'s `test-util` feature so that crates downstream of it can build
against the same mocks — `#[cfg(test)]` does not cross a crate boundary. A
crate that needs them declares `icp-project = { workspace = true, features =
["test-util"] }` as a dev-dependency, as `icp-app` does. Nothing behind the
feature ships in a normal build.
