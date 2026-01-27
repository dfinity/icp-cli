# Project Model

This document explains how icp-cli discovers, loads, and consolidates your project configuration.

## Project Structure

An icp-cli project is any directory containing an `icp.yaml` file. This file is the root of your project configuration.

The project layout is flexible but a typical layout will look like the one below. Notice that some of the directories
and configuration files are marked as optional: This is because the configuration can be split across multiple files
or inlined inside `icp.yaml`

```
my-project/
├── icp.yaml              # Project configuration
├── networks/             # [Optional] network manifests
│    ├── testnet1.yaml
│    └── testnet2.yaml
├── environments/         # [Optional] environment manifests
│    ├── dev.yaml
│    ├── production.yaml
│    └── staging.yaml
└── src/                  # Canister source code
    └── canisters/
        ├── frontend/
        │   └── canister.yaml  # [Optional] canister manifest
        └── backend/
            └── canister.yaml  # [Optional] canister manifest
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

## Network Discovery

Networks can be defined in three ways:

### Implicit networks

There are two implicit networks defined:
- `local` - is a local managed network
- `ic` - is the IC mainnet (connected network)

Their configuration is equivalent to:

```yaml
networks:
  - name: ic
    configuration:
      mode: connected
      url: https://icp-api.io
  - name: local
    configuration:
      mode: managed
      gateway:
        host: localhost
        port: 8000
```

### Inline Definition

Define networks directly in `icp.yaml`

```yaml
networks:
  - name: testnet
    configuration:
      mode: connected
      url: https://my-icp-testnet.io
      root-key: <some root key>
```

### External Files

Reference separate YAML files

```yaml
networks:
  - networks/testnet1.yaml
  - networks/testnet2.yaml

```

## Environment Discovery

Environments can be defined in three ways:

### Implicit Environments

There are two implicit environments:
- `local` - uses the local managed network
- `ic` - uses the IC mainnet

They are defined like this:

```yaml
environments:
  - name: local
    network: local
  - name: ic
    network: ic
```

### Inline Definition

Define environments directly in `icp.yaml`

```yaml
environments:
  - name: my-staging-env
    network: mainnet
  - name: my-production-env
    network: mainnet

```

### External Files

Reference separate YAML files

```yaml
environments:
  - env/my-staging-env.yaml
  - env/my-production-env.yaml

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
  - frontend  # look for frontend/canister.yaml 
  - backend   # look for backend/canister.yaml 
```

### Glob Patterns

Discover canisters automatically:

```yaml
canisters:
  - canisters/*         # find all `canister.yaml` files in canisters/**
```

## Configuration Consolidation

icp-cli consolidates configuration from multiple sources into a single effective configuration. The order of precedence (highest to lowest):

1. **Environment-specific settings** — Override everything for that environment
2. **Canister-level settings** — Default settings for a canister
3. **Recipe-generated configuration** — Expanded from recipe templates
4. **Implicit defaults** — Built-in networks and environments

View the effective configuration:

```bash
# outputs the effective project configuration in yaml
icp project show

# You can use yq to view the effective settings of a canister
# in a particular environment. Here we're looking at the settings
# in the 'local' environment
icp project show | yq -r ".environments.local"
 
```

## Generated Files

icp-cli creates a `.icp/` directory in your project root to store build artifacts, canister IDs, and network state.

```
<project-root>/.icp/
├── cache/                    # Temporary/recreatable data
│   ├── artifacts/            # Built WASM files
│   ├── mappings/             # Canister IDs for managed networks
│   └── networks/             # Local network state
└── data/
    └── mappings/             # Canister IDs for connected networks
```

### What's safe to delete

| Directory | Safe to delete? | Consequence |
|-----------|-----------------|-------------|
| `.icp/cache/` | **Yes** | Local network state and local canister IDs are recreated on next deploy. Built WASMs are rebuilt. |
| `.icp/data/` | **No** | Contains mainnet canister ID mappings. Deleting means icp-cli won't know which canisters you've deployed (though the canisters still exist on-chain). |

### Version control

Add to `.gitignore`:
```gitignore
.icp/cache/
```

Consider tracking `.icp/data/` in version control to preserve mainnet canister ID mappings. Losing these mappings means you'll need to manually look up your canister IDs on the IC dashboard.

## Canister IDs

When you deploy, icp-cli records canister IDs in mapping files. The location depends on the network type:

- **Managed networks** (eg: local): `.icp/cache/mappings/<environment>.ids.json`
- **Connected networks** (eg: mainnet): `.icp/data/mappings/<environment>.ids.json`

Each environment maintains separate canister IDs, so your local deployment and mainnet deployment have different IDs.

The mapping file for managed networks is ephemeral, meaning that it will be removed when the network is stopped.

## Project Root Detection

icp-cli looks for `icp.yaml` in the current directory and parent directories. You can override this:

```bash
icp deploy --project-root-override /path/to/project
```

## Next Steps

- [Build, Deploy, Sync](build-deploy-sync.md) — The deployment lifecycle

[Browse all documentation →](../index.md)
