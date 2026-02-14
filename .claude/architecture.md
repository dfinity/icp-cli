# Architecture Details

## Project Model

The project model is built hierarchically through manifest consolidation:

1. **Project Manifest** (`icp.yaml`): Root configuration defining canisters, networks, and environments
2. **Canister Manifest** (`canister.yaml`): Per-canister configuration for build and sync steps
3. **Consolidated Project**: Final `Project` struct combining all manifests into a unified view

Key types in `crates/icp/src/lib.rs`:
- `Project`: Contains all canisters, networks, and environments
- `Environment`: Links a network with a set of canisters
- `Network`: Configuration for local (managed) or remote (connected) networks
- `Canister`: Build and sync configuration for a single canister

## Manifest System

Manifests are YAML files that define project structure. The system supports:

- **Inline definitions**: Define resources directly in `icp.yaml`
- **Path references**: Reference external manifest files
- **Glob patterns**: For canisters, use globs like `canisters/*` to auto-discover

The `consolidate_manifest` function in `crates/icp/src/project.rs` transforms raw manifests into the final `Project` structure. The serde structs in the `icp::manifest` module represent the format that the user's YAML files can be written in, while the serde structs with identical meaning outside `icp::manifest` are instead the canonical form, with defaults filled in and normalizations applied. Code should always deal with the canonical form.

## Build Adapters

Canisters are built using adapter pipelines defined in `crates/icp/src/manifest/adapter/`:

- **Script Adapter**: Runs shell commands with environment variables (e.g., `$ICP_WASM_OUTPUT_PATH`)
- **Prebuilt Adapter**: Uses pre-compiled WASM from local files, URLs, or registry
- **Assets Adapter**: Packages static assets for frontend canisters

Build steps are executed sequentially in `crates/icp/src/canister/build/`.

## Recipe System

Recipes are Handlebars templates that generate build/sync configuration. Implementation in `crates/icp/src/canister/recipe/`:

- **Registry recipes**: `@dfinity/rust@v3.0.0` resolves to GitHub releases URL
- **Local recipes**: `file://path/to/recipe.hbs`
- **Remote recipes**: Direct URLs with SHA256 verification

The `@dfinity` prefix is hardcoded to `https://github.com/dfinity/icp-cli-recipes/releases/download/{recipe}-{version}/recipe.hbs`

## Network Management

Two network types in `crates/icp/src/network/`:

- **Managed Networks**: Local test networks launched via `icp-cli-network-launcher` (wraps PocketIC)
- **Connected Networks**: Remote networks (mainnet, testnets) accessed via HTTP

### Implicit Networks and Environments

The CLI provides two implicit networks and environments that are always available:

- **`local` network**: A default managed network on `localhost:8000`. Users can override this in their `icp.yaml` to customize the local development environment (e.g., different port or connecting to an existing network).
- **`ic` network**: The IC mainnet at `https://icp-api.io`. This network is **protected** and cannot be overridden to prevent accidental production deployment with incorrect settings.

Corresponding implicit environments are also provided:
- **`local` environment**: Uses the `local` network with all project canisters. This is the default environment when none is specified.
- **`ic` environment**: Uses the `ic` network with all project canisters.

These constants are defined in `crates/icp/src/prelude.rs` as `LOCAL` and `IC` and are used throughout the codebase.

## Identity & Canister IDs

- **Identities**: Stored in platform-specific directories as PEM files (Secp256k1 or Ed25519):
  - macOS: `~/Library/Application Support/org.dfinity.icp-cli/identity/`
  - Linux: `~/.local/share/icp-cli/identity/`
  - Windows: `%APPDATA%\icp-cli\data\identity\`
  - Override with `ICP_HOME` environment variable: `$ICP_HOME/identity/`
- **Canister IDs**: Persisted in `.icp/{cache,data}/mappings/<environment>.ids.json` within project directories
  - Managed networks (local) use `.icp/cache/mappings/`
  - Connected networks (mainnet) use `.icp/data/mappings/`

Store management is in `crates/icp/src/store_id.rs`.
