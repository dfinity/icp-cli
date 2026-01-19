# Configuration Reference

Complete reference for `icp.yaml` project configuration.

For conceptual explanation, see [Project Model](../concepts/project-model.md).

## File Structure

```yaml
# icp.yaml
canisters:
  - # canister definitions or references

networks:
  - # network definitions or references (optional)

environments:
  - # environment definitions (optional)
```

## Canisters

### Inline Definition

```yaml
canisters:
  - name: my-canister
    build:
      steps:
        - type: script
          commands:
            - echo "Building..."
    sync:
      steps:
        - type: assets
          source: www
          target: /
    settings:
      compute_allocation: 5
    init_args: "()"
```

### External Reference

```yaml
canisters:
  - path/to/canister.yaml
  - canisters/*           # Glob pattern
  - services/**/*.yaml    # Recursive glob
```

### Canister Properties

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `name` | string | Yes | Unique canister identifier |
| `build` | object | Yes | Build configuration |
| `sync` | object | No | Post-deployment sync configuration |
| `settings` | object | No | Canister settings |
| `init_args` | string | No | Initialization arguments (Candid or hex) |
| `recipe` | object | No | Recipe reference (alternative to build) |

## Build Steps

### Script Step

Execute shell commands:

```yaml
build:
  steps:
    - type: script
      commands:
        - cargo build --target wasm32-unknown-unknown --release
        - cp target/wasm32-unknown-unknown/release/my_canister.wasm "$ICP_WASM_OUTPUT_PATH"
```

**Environment variables:**
- `ICP_WASM_OUTPUT_PATH` — Target path for WASM output
- `ICP_PROJECT_ROOT` — Project root directory

### Pre-built Step

Use existing WASM:

```yaml
build:
  steps:
    - type: pre-built
      path: dist/canister.wasm
      sha256: abc123...  # Optional integrity check
```

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `path` | string | Yes | Path to WASM file |
| `sha256` | string | No | SHA256 hash for verification |

### Assets Step

Bundle static files:

```yaml
build:
  steps:
    - type: assets
      source: www
      target: /
      include_patterns:
        - "*.html"
        - "*.js"
      exclude_patterns:
        - "*.map"
```

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `source` | string | Yes | Source directory |
| `target` | string | Yes | Target path in canister |
| `include_patterns` | array | No | Glob patterns to include |
| `exclude_patterns` | array | No | Glob patterns to exclude |

## Sync Steps

### Assets Sync

Upload files to asset canister:

```yaml
sync:
  steps:
    - type: assets
      source: dist
      target: /
```

Same properties as assets build step.

## Recipes

### Recipe Reference

```yaml
canisters:
  - name: my-canister
    recipe:
      type: "@dfinity/rust"
      sha256: abc123...  # Required for remote URLs
      configuration:
        package: my-crate
```

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `type` | string | Yes | Recipe source (registry, URL, or local path) |
| `sha256` | string | Conditional | Required for remote URLs |
| `configuration` | object | No | Parameters passed to recipe template |

### Recipe Type Formats

```yaml
# Registry (recommended)
type: "@dfinity/rust"
type: "@dfinity/rust@v1.0.0"  # With version

# Local file
type: ./recipes/my-recipe.hb.yaml

# Remote URL
type: https://example.com/recipe.hb.yaml
```

## Networks

### Managed Network

```yaml
networks:
  - name: local-dev
    mode: managed
    gateway:
      host: 127.0.0.1
      port: 4943
```

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `name` | string | Yes | Network identifier |
| `mode` | string | Yes | `managed` |
| `gateway.host` | string | No | Host address (default: 127.0.0.1) |
| `gateway.port` | integer | No | Port number (default: 8000) |

### Connected Network

```yaml
networks:
  - name: testnet
    mode: connected
    url: https://testnet.ic0.app
    root-key: <hex-encoded-key>  # For non-mainnet
```

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `name` | string | Yes | Network identifier |
| `mode` | string | Yes | `connected` |
| `url` | string | Yes | Network endpoint URL |
| `root-key` | string | No | Hex-encoded root key (non-mainnet only) |

### Docker Network

```yaml
networks:
  - name: docker-local
    mode: managed
    image: ghcr.io/dfinity/icp-cli-network-launcher
    port-mapping:
      - "0:4943"
```

See [Containerized Networks](../containers.md) for full options.

## Environments

```yaml
environments:
  - name: staging
    network: ic
    canisters:
      - frontend
      - backend
    settings:
      frontend:
        memory_allocation: 2147483648
      backend:
        compute_allocation: 10
        environment_variables:
          LOG_LEVEL: "info"
    init_args:
      backend: "(record { mode = \"staging\" })"
```

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `name` | string | Yes | Environment identifier |
| `network` | string | Yes | Network to deploy to |
| `canisters` | array | No | Canisters to include (default: all) |
| `settings` | object | No | Per-canister setting overrides |
| `init_args` | object | No | Per-canister init arg overrides |

## Canister Settings

See [Canister Settings Reference](canister-settings.md) for all options.

```yaml
settings:
  compute_allocation: 5
  memory_allocation: 4294967296
  freezing_threshold: 2592000
  reserved_cycles_limit: 1000000000000
  wasm_memory_limit: 1073741824
  wasm_memory_threshold: 536870912
  log_visibility: controllers
  environment_variables:
    KEY: "value"
```

## Init Args

Candid text format:

```yaml
init_args: "(record { owner = principal \"aaaaa-aa\" })"
```

Hex-encoded bytes:

```yaml
init_args: "4449444c016d7b0100010203"
```

## Implicit Defaults

### Networks

| Name | Mode | Description |
|------|------|-------------|
| `local` | managed | `localhost:8000`, can be overridden |
| `ic` | connected | ICP mainnet, cannot be overridden |

### Environments

| Name | Network | Canisters |
|------|---------|-----------|
| `local` | local | All |
| `ic` | ic | All |

## Complete Example

```yaml
canisters:
  - name: frontend
    recipe:
      type: "@dfinity/assets"
      configuration:
        source: dist
    settings:
      memory_allocation: 1073741824

  - name: backend
    build:
      steps:
        - type: script
          commands:
            - cargo build --target wasm32-unknown-unknown --release
            - cp target/wasm32-unknown-unknown/release/backend.wasm "$ICP_WASM_OUTPUT_PATH"
    settings:
      compute_allocation: 5
    init_args: "(record { admin = principal \"aaaaa-aa\" })"

networks:
  - name: local
    mode: managed
    gateway:
      port: 9999

environments:
  - name: staging
    network: ic
    canisters: [frontend, backend]
    settings:
      backend:
        compute_allocation: 10
        environment_variables:
          ENV: "staging"

  - name: production
    network: ic
    canisters: [frontend, backend]
    settings:
      frontend:
        memory_allocation: 4294967296
      backend:
        compute_allocation: 30
        freezing_threshold: 7776000
        environment_variables:
          ENV: "production"
    init_args:
      backend: "(record { admin = principal \"xxxx-xxxx\" })"
```

## Schema

JSON schemas for editor integration are available in [docs/schemas/](../schemas/).

Configure your editor to use them:

```yaml
# yaml-language-server: $schema=./docs/schemas/icp-yaml-schema.json
canisters:
  - name: my-canister
    # ...
```
