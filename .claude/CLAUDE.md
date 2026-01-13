# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`icp-cli` is a command-line tool for developing and deploying applications on the Internet Computer Protocol (ICP).

## Essential Commands

### Building & Testing

```bash
# Build all crates in workspace
cargo build

# Build in release mode
cargo build --release

# Build only the CLI binary
cargo build --bin icp

# Run all tests (network launcher is auto-downloaded on first run)
cargo test

# Run tests for specific package
cargo test -p icp-cli

# Run a specific test
cargo test <test_name>

# Run with verbose output
cargo test -- --nocapture
```

### Development Workflow

```bash
# Add CLI to path for testing
export PATH=$(pwd)/target/debug:$PATH

# Check if CLI works
icp help

# if the commands have changed, generate CLI documentation:
./scripts/generate-cli-docs.sh


# if the manifest types change regenerate the schema:
./scripts/generate-config-schema.sh

# After making changes and if the tests pass run cargo fmt and cargo clippy:
cargo fmt && cargo clippy
```

## Architecture

### Workspace Structure

This is a Rust workspace with multiple crates:

- **`crates/icp-cli`**: Main CLI binary (`icp`) with command implementations
- **`crates/icp`**: Core library with project model, manifest loading, canister management, network configuration
- **`crates/icp-canister-interfaces`**: Canister interface definitions for ICP system canisters
- **`crates/schema-gen`**: JSON schema generation for manifest validation

### Core Concepts

#### Project Model

The project model is built hierarchally through manifest consolidation:

1. **Project Manifest** (`icp.yaml`): Root configuration defining canisters, networks, and environments
2. **Canister Manifest** (`canister.yaml`): Per-canister configuration for build and sync steps
3. **Consolidated Project**: Final `Project` struct combining all manifests into a unified view

Key types in `crates/icp/src/lib.rs`:
- `Project`: Contains all canisters, networks, and environments
- `Environment`: Links a network with a set of canisters
- `Network`: Configuration for local (managed) or remote (connected) networks
- `Canister`: Build and sync configuration for a single canister

#### Manifest System

Manifests are YAML files that define project structure. The system supports:

- **Inline definitions**: Define resources directly in `icp.yaml`
- **Path references**: Reference external manifest files
- **Glob patterns**: For canisters, use globs like `canisters/*` to auto-discover

The `consolidate_manifest` function in `crates/icp/src/project.rs` transforms raw manifests into the final `Project` structure.

#### Build Adapters

Canisters are built using adapter pipelines defined in `crates/icp/src/manifest/adapter/`:

- **Script Adapter**: Runs shell commands with environment variables (e.g., `$ICP_WASM_OUTPUT_PATH`)
- **Prebuilt Adapter**: Uses pre-compiled WASM from local files, URLs, or registry
- **Assets Adapter**: Packages static assets for frontend canisters

Build steps are executed sequentially in `crates/icp/src/canister/build/`.

#### Network Management

Two network types in `crates/icp/src/network/`:

- **Managed Networks**: Local test networks launched via `icp-cli-network-launcher` (wraps PocketIC)
- **Connected Networks**: Remote networks (mainnet, testnets) accessed via HTTP

The network launcher is automatically downloaded on first use. For development/debugging, you can override with `ICP_CLI_NETWORK_LAUNCHER_PATH`.

##### Network Overrides

- Users can override the "local" network definition in their `icp.yaml` to customize the local development environment
- The "mainnet" network is protected and cannot be overridden to prevent production deployment accidents
- If no "local" network is defined, a default managed network on `localhost:8000` is automatically added

#### Identity & Canister IDs

- **Identities**: Stored in `~/.config/icp/identity/` as PEM files (Secp256k1 or Ed25519)
- **Canister IDs**: Persisted in `.icp/data/<network-name>/canister_ids.json` within project directories

Store management is in `crates/icp/src/store_id.rs`.

### Command Structure

Commands are organized in `crates/icp-cli/src/commands/`:

- Each command/subcommand is a module with an `exec()` function
- Commands receive a `Context` (from `crates/icp/src/context/`) containing:
  - Project loader for lazy manifest loading
  - Terminal for user interaction
  - Configuration settings
- The main router in `main.rs` dispatches commands using `clap`

### Key Patterns

- **Async/Await**: Heavy use of `tokio` for async operations (network calls, file I/O)
- **Error Handling**: Uses `snafu` for structured error types with context
- **Dependency Injection**: Traits like `ProjectLoad`, `ProjectRootLocate` enable testing with mocks
- **Lazy Loading**: Project manifests are loaded on-demand to avoid unnecessary work

## Testing

### Test Structure

Tests are split between unit tests (in modules) and integration tests:

- Integration tests in `crates/icp-cli/tests/` test full command execution
- Use `assert_cmd` for CLI assertions and `predicates` for output matching
- Use `serial_test` with file locks for tests that share resources (network ports)

### Test Requirements

- The network launcher is automatically downloaded on first test run
- Some tests launch local networks and require available ports

### Mock Helpers

`crates/icp/src/lib.rs` provides test utilities:

- `MockProjectLoader::minimal()`: Single canister, network, environment
- `MockProjectLoader::complex()`: Multiple canisters, networks, environments
- `NoProjectLoader`: Simulates missing project for error cases

## Important Constraints

### Rust Edition & Toolchain

- Uses **Rust 2024 edition** (requires Rust 1.88.0+)
- Update `rust-version` in workspace `Cargo.toml` when changing

### Network Launcher Dependency

- The network launcher is automatically downloaded on first use (both CLI and tests)
- `ICP_CLI_NETWORK_LAUNCHER_PATH` can be set to override the auto-downloaded version for debugging
- Manual download available from: github.com/dfinity/icp-cli-network-launcher/releases

### Schema Generation

The project includes JSON schemas for manifest validation:

- Schema is generated in `crates/schema-gen/`
- Referenced in `icp.yaml` files via `# yaml-language-server: $schema=...`
- Regenerate when manifest types change by running: `./scripts/generate-config-schema.sh`

### Docs generation

- The cli reference is generated in `docs/cli-reference.md`.
- Regenerate the cli reference when commands changes by running: `./scripts/generate-cli-docs.sh`

### Paths

All paths are UTF-8. `PathBuf` and `Path` are the types from `camino`.

- You do not need to add `.display()` to use them in format strings
- Do not import `Path` or `PathBuf` from `std`; if those names are not available, glob-import `icp::prelude::*` (or `crate::prelude::*` if in `icp`).

### Error handling

This project uses Snafu for error handling.

- Every new *primary erroring action* gets its own error variant. There is no `MyError::Io { source: io::Error }`, instead (hypothetically) `OpenSocket` and `WriteSocket` should be separate. `snafu(context(false))` is not permitted. `snafu(transparent)` should *only* be used for source error types defined elsewhere in this repo, *not* for foreign error types.
- Every error regarding a file in some way (processing, creating, etc.) should contain the file path of the error. It is okay to add 'dummy' file path parameters only used in error handling routes. For 'basic' file ops and JSON/YML loading use the functions in `icp::fs`, whose errors include the file path and can be made `snafu(transparent)`. 

## Examples

The `examples/` directory contains working project templates demonstrating:

- `icp-motoko/`: Motoko canister with script adapter
- `icp-rust/`: Rust canister compilation
- `icp-multi-canister/`: Multi-canister projects with environments
- `icp-network-connected/`: Remote network configuration
- `icp-pre-built/`: Using prebuilt WASM files

These serve as integration tests and documentation for users and must be kept up to date.
