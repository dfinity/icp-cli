# Project Model

This document explains how icp-cli discovers, loads, and consolidates your project configuration.

## Project Structure

An icp-cli project is any directory containing an `icp.yaml` file. This file is the root of your project configuration.

```
my-project/
├── icp.yaml              # Project configuration
├── src/                  # Source code
├── canisters/            # Optional: external canister configs
│   ├── frontend.yaml
│   └── backend.yaml
└── .icp/                 # Generated: canister IDs, build artifacts
    ├── cache/            # For managed networks (local)
    │   └── mappings/
    │       └── local.ids.json
    └── data/             # For connected networks (mainnet)
        └── mappings/
            └── ic.ids.json
```

## The icp.yaml File

The `icp.yaml` file defines:

- **Canisters** — What to build and deploy
- **Networks** — Where to deploy (optional, defaults provided)
- **Environments** — Named deployment configurations (optional, defaults provided)

Minimal example:

```yaml
canisters:
  - name: hello
    build:
      steps:
        - type: script
          commands:
            - cargo build --target wasm32-unknown-unknown --release
            - cp target/wasm32-unknown-unknown/release/hello.wasm "$ICP_WASM_OUTPUT_PATH"
```

## Canister Discovery

Canisters can be defined in three ways:

### Inline Definition

Define canisters directly in `icp.yaml`:

```yaml
canisters:
  - name: my-canister
    build:
      steps:
        - type: script
          commands:
            - echo "Building..."
```

### External Files

Reference separate YAML files:

```yaml
canisters:
  - frontend/canister.yaml
  - backend/canister.yaml
```

### Glob Patterns

Discover canisters automatically:

```yaml
canisters:
  - canisters/*         # All .yaml files in canisters/
  - services/**/*.yaml  # Recursive search
```

## Configuration Consolidation

icp-cli consolidates configuration from multiple sources into a single effective configuration. The order of precedence (highest to lowest):

1. **Environment-specific settings** — Override everything for that environment
2. **Canister-level settings** — Default settings for a canister
3. **Recipe-generated configuration** — Expanded from recipe templates
4. **Implicit defaults** — Built-in networks and environments

View the effective configuration:

```bash
icp project show
```

## Implicit Defaults

icp-cli provides sensible defaults so minimal configuration works:

### Implicit Networks

- `local` — A managed network on `localhost:8000`
- `mainnet` — The Internet Computer mainnet (protected, cannot be overridden)

### Implicit Environments

- `local` — Uses the `local` network, includes all canisters
- `ic` — Uses `mainnet`, includes all canisters

## Overriding Defaults

Override the local network configuration:

```yaml
networks:
  - name: local
    mode: managed
    gateway:
      port: 9999  # Custom port
```

Add custom environments:

```yaml
environments:
  - name: staging
    network: mainnet
    canisters: [frontend, backend]
```

## Canister IDs

When you deploy, icp-cli records canister IDs in mapping files. The location depends on the network type:

- **Managed networks** (local): `.icp/cache/mappings/<environment>.ids.json`
- **Connected networks** (mainnet): `.icp/data/mappings/<environment>.ids.json`

Each environment maintains separate canister IDs, so your local deployment and mainnet deployment have different IDs.

## Project Root Detection

icp-cli looks for `icp.yaml` in the current directory and parent directories. You can override this:

```bash
icp deploy --project-root-override /path/to/project
```

## Next Steps

- [Build, Deploy, Sync](build-deploy-sync.md) — The deployment lifecycle

[Browse all documentation →](../index.md)
