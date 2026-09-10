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

- **`crates/icp-cli`**: Main CLI binary (`icp`): argument parsing, command implementations, and all terminal presentation
- **`crates/icp-app`**: Everything about the machine the tool runs on: identities and the keyring, user settings, the global directory layout, the package cache, local networks and the launcher that runs them, telemetry, offline message signing, and the operations that act on a canister by principal
- **`crates/icp-project`**: Everything about a project: the project model, manifest loading and consolidation, canister management, and the operations that build, install, sync and deploy
- **`crates/icp-sync-plugin`**: The wasmtime Component Model runtime for sync plugins — one implementation of `icp-project`'s plugin-runner seam
- **`crates/icp-events`**: Typed progress events passed from operations to the CLI's renderers
- **`crates/icp-canister-interfaces`**: Canister interface definitions for ICP system canisters
- **`crates/schema-gen`**: JSON schema generation for manifest validation

### The app/project boundary

`icp-app` depends on `icp-project`, and never the other way round. `icp-cli`
depends on both directly — `icp-app` re-exports nothing, so a command reaches
for the crate that owns what it needs.

Which side a thing belongs on is decided by its inputs: anything driven by the
project model (a `Canister`, an `Environment`, a manifest) is `icp-project`;
anything keyed by a bare `Principal`, or by global state on this machine, is
`icp-app`. Low-level primitives sink rather than float: a management-canister
wrapper lives in `icp-project` whenever `icp-project` itself has to issue it,
even though it takes a principal, and `icp-app` calls down into it.

`icp-project` is meant to end up runnable inside a canister, so it must not
reach the host directly. What it needs from the machine it asks for through a
trait declared there and implemented elsewhere — in `icp-app`, or in
`icp-sync-plugin`, which likewise depends on `icp-project` and never the
reverse:

- `files::FileSystem` — the files a project is made of
- `calls::CanisterCalls` — submitting a call, and reading a certified fact
- `network::Access` — a network's endpoints, root key and friendly domains
- `canister::wasm::Fetch` — a wasm module a manifest names by URL
- `canister::recipe::Resolve` — a recipe's Handlebars template
- `canister::sync::plugin::Run` — running one sync plugin
- `canister::sync::script::ScriptRunner` — running one sync script
- `store_id::Access` / `store_artifact::Access` — the project's `.icp` stores
- `host::Observe` — what resolution turned up, for telemetry

Host implementations of the first, the last two stores and the script runner
ship in `icp-project` itself behind the default-on `host` feature; the rest
have no implementation there at all.

Because those are implemented across a crate boundary, their error types carry
their cause boxed and pass it through with `#[snafu(transparent)]`, which leaves
the wrapper out of the source chain so the cause is reported once rather than
both restated as the wrapper's message and reported beneath it.

This is the one exception to the error-handling rule below, and to both of its
halves: on a trait whose implementation the crate cannot name there is no
variant to write, and what `transparent` passes through here is a boxed foreign
error rather than one of this repo's.

### Command Structure

Commands are in `crates/icp-cli/src/commands/`, each as a module with an `exec()` function receiving a `Context` (from `crates/icp-app/src/context/`). Dispatched via `clap` in `main.rs`. Traits like `ProjectLoad` and `ProjectRootLocate` enable dependency injection for testing.

### Commands vs. operations

Commands own the user experience: parsing arguments, choosing a renderer, and printing results. The real work lives in `crates/icp-project/src/operations/` (and `crates/icp-app/src/operations/` for the principal-keyed ones) — building, syncing, installing, and so on. Operations never print or draw: they report progress as [`icp-events`](crates/icp-events) events through a `Reporter` and return typed errors, and `crates/icp-cli/src/render/` decides how any of it looks. Anything a command needs to display comes back as event data or a return value, never as a formatted string from an operation.

An operation takes `&icp_project::host::Host` — the project-side seams and the
environment/canister-id resolution built on them — plus whatever its caller
resolved for it (an agent, install arguments). It does not take `Context`:
identities, the keyring and the global directories are no business of an
operation's.

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
- Do not import `Path` or `PathBuf` from `std`; if those names are not available, glob-import `icp_project::prelude::*` (or `crate::prelude::*` if in `icp-project`).

### Error handling

This project uses Snafu for error handling.

- Every new *primary erroring action* gets its own error variant. There is no `MyError::Io { source: io::Error }`, instead (hypothetically) `OpenSocket` and `WriteSocket` should be separate. `snafu(context(false))` is not permitted. `snafu(transparent)` should *only* be used for source error types defined elsewhere in this repo, *not* for foreign error types.
- Every error regarding a file in some way (processing, creating, etc.) should contain the file path of the error. It is okay to add 'dummy' file path parameters only used in error handling routes. For 'basic' file ops and JSON/YML loading use the functions in `icp_project::fs`, whose errors include the file path and can be made `snafu(transparent)`.

## Documentation & Examples

See `.claude/docs-guidelines.md` for documentation structure, installation instructions guidance, and schema/CLI docs generation.

See `.claude/recipe-docs.md` for recipe documentation verification rules and cross-repository checks.

The `examples/` directory contains working project templates that serve as integration tests and must be kept up to date.
