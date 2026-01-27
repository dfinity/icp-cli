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
          dir: www
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

See [Environment Variables Reference](environment-variables.md) for all available variables.

### Pre-built Step

Use existing WASM from a local file or remote URL:

```yaml
# Local file
build:
  steps:
    - type: pre-built
      path: dist/canister.wasm
      sha256: abc123...  # Optional integrity check

# Remote URL
build:
  steps:
    - type: pre-built
      url: https://github.com/example/releases/download/v1.0/canister.wasm
      sha256: abc123...  # Recommended for remote files
```

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `path` | string | One of `path` or `url` | Local path to WASM file |
| `url` | string | One of `path` or `url` | URL to download WASM file from |
| `sha256` | string | No | SHA256 hash for verification (recommended for URLs) |

## Sync Steps

Sync steps run after canister deployment to configure the running canister.

### Assets Sync

Upload files to asset canister:

```yaml
# Single directory
sync:
  steps:
    - type: assets
      dir: dist

# Multiple directories
sync:
  steps:
    - type: assets
      dirs:
        - dist
        - static
        - public/images
```

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `dir` | string | One of `dir` or `dirs` | Single directory to upload |
| `dirs` | array | One of `dir` or `dirs` | Multiple directories to upload |

### Script Sync

Run shell commands after deployment:

```yaml
sync:
  steps:
    - type: script
      commands:
        - echo "Post-deployment setup"
        - ./scripts/configure-canister.sh
```

Script sync steps support the same `command` and `commands` fields as build script steps.

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
| `gateway.host` | string | No | Host address (default: localhost) |
| `gateway.port` | integer | No | Port number (default: 8000, use 0 for random) |
| `artificial_delay_ms` | integer | No | Artificial delay to add to every update call (ms) |
| `ii` | boolean | No | Set up Internet Identity canister (default: false) |
| `nns` | boolean | No | Set up NNS canisters (default: false) |
| `subnets` | array | No | Configure subnet types (default: one application subnet) |

#### Subnet Configuration

Configure the local network's subnet layout. By default, a single application subnet is created. Use multiple subnets to test cross-subnet (Xnet) calls:

```yaml
networks:
  - name: local
    mode: managed
    subnets:
      - application
      - application
      - application
```

Available subnet types: `application`, `system`, `verified-application`, `bitcoin`, `fiduciary`, `nns`, `sns`

**Note:** Subnet type support depends on the network launcher version. The `application` type is commonly used for testing.

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

See [Containerized Networks](../guides/containerized-networks.md) for full options.

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
      type: "@dfinity/asset-canister"
      configuration:
        dir: dist
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

JSON schemas for editor integration are available in [docs/schemas/](https://github.com/dfinity/icp-cli/tree/main/docs/schemas):
- [`icp-yaml-schema.json`](https://raw.githubusercontent.com/dfinity/icp-cli/main/docs/schemas/icp-yaml-schema.json) - Main project configuration
- [`canister-yaml-schema.json`](https://raw.githubusercontent.com/dfinity/icp-cli/main/docs/schemas/canister-yaml-schema.json) - Canister configuration
- [`network-yaml-schema.json`](https://raw.githubusercontent.com/dfinity/icp-cli/main/docs/schemas/network-yaml-schema.json) - Network configuration
- [`environment-yaml-schema.json`](https://raw.githubusercontent.com/dfinity/icp-cli/main/docs/schemas/environment-yaml-schema.json) - Environment configuration

Configure your editor to use them for autocomplete and validation:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/dfinity/icp-cli/main/docs/schemas/icp-yaml-schema.json
canisters:
  - name: my-canister
    # ...
```
