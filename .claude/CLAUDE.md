# CLAUDE.md

## Project Overview

`icp-cli` is a command-line tool for developing and deploying applications on the Internet Computer Protocol (ICP).

## Essential Commands

```bash
cargo build --bin icp                # Build the CLI binary
cargo test                           # Run all tests (launcher auto-downloads on first run)
cargo test -p icp-cli                # Tests for a specific package
cargo test --test <file> -- <name>   # Specific test
cargo fmt && cargo clippy            # Run after changes pass tests
./scripts/generate-cli-docs.sh       # Regenerate CLI docs when commands change
./scripts/generate-config-schemas.sh # Regenerate schema when manifest types change
```

## Architecture

### Workspace Structure

- **`crates/icp-cli`**: Main CLI binary (`icp`) with command implementations
- **`crates/icp`**: Core library with project model, manifest loading, canister management, network configuration
- **`crates/icp-canister-interfaces`**: Canister interface definitions for ICP system canisters
- **`crates/schema-gen`**: JSON schema generation for manifest validation

### Command Structure

Commands are in `crates/icp-cli/src/commands/`, each as a module with an `exec()` function receiving a `Context` (from `crates/icp/src/context/`). Dispatched via `clap` in `main.rs`. Traits like `ProjectLoad` and `ProjectRootLocate` enable dependency injection for testing.

See `.claude/architecture.md` for detailed subsystem documentation (manifests, build adapters, recipes, networks, identity).

See `.claude/testing.md` for test structure, mock helpers, and test requirements.

## Important Constraints

### Rust Edition & Toolchain

- Uses **Rust 2024 edition** (requires Rust 1.88.0+)
- Update `rust-version` in workspace `Cargo.toml` when changing

### Network Launcher

The `icp-cli-network-launcher` (wraps PocketIC) is automatically downloaded on first use, both for the CLI and tests. Override with `ICP_CLI_NETWORK_LAUNCHER_PATH` for debugging.

### Paths

All paths are UTF-8. `PathBuf` and `Path` are the types from `camino`.

- You do not need to add `.display()` to use them in format strings
- Do not import `Path` or `PathBuf` from `std`; if those names are not available, glob-import `icp::prelude::*` (or `crate::prelude::*` if in `icp`).

### Error handling

This project uses Snafu for error handling.

- Every new *primary erroring action* gets its own error variant. There is no `MyError::Io { source: io::Error }`, instead (hypothetically) `OpenSocket` and `WriteSocket` should be separate. `snafu(context(false))` is not permitted. `snafu(transparent)` should *only* be used for source error types defined elsewhere in this repo, *not* for foreign error types.
- Every error regarding a file in some way (processing, creating, etc.) should contain the file path of the error. It is okay to add 'dummy' file path parameters only used in error handling routes. For 'basic' file ops and JSON/YML loading use the functions in `icp::fs`, whose errors include the file path and can be made `snafu(transparent)`.

## Documentation & Examples

See `.claude/docs-guidelines.md` for documentation structure, installation instructions guidance, and schema/CLI docs generation.

See `.claude/recipe-docs.md` for recipe documentation verification rules and cross-repository checks.

The `examples/` directory contains working project templates that serve as integration tests and must be kept up to date.
