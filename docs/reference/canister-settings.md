# Canister Settings Reference

Complete reference for all canister settings available in icp-cli.

Canister settings control resource allocation, behavior, and runtime configuration. They can be specified:

1. At the **canister level** in `icp.yaml` or `canister.yaml`
2. At the **environment level** to override per-environment

## Settings

### compute_allocation

Guaranteed percentage of compute capacity.

| Property | Value |
|----------|-------|
| Type | Integer |
| Range | 0-100 |
| Default | 0 (best effort) |

```yaml
settings:
  compute_allocation: 10
```

Higher values guarantee more compute but cost more cycles.

### memory_allocation

Fixed memory reservation in bytes.

| Property | Value |
|----------|-------|
| Type | Integer |
| Unit | Bytes |
| Default | Dynamic allocation |

```yaml
settings:
  memory_allocation: 4294967296  # 4GB
```

If not set, the canister uses dynamic memory allocation.

### freezing_threshold

Time in seconds before the canister freezes due to low cycles.

| Property | Value |
|----------|-------|
| Type | Integer |
| Unit | Seconds |
| Default | 2592000 (30 days) |

```yaml
settings:
  freezing_threshold: 7776000  # 90 days
```

The canister freezes if its cycles balance would be exhausted within this threshold.

### reserved_cycles_limit

Maximum cycles the canister can hold in reserve.

| Property | Value |
|----------|-------|
| Type | Integer |
| Unit | Cycles |
| Default | No limit |

```yaml
settings:
  reserved_cycles_limit: 1000000000000  # 1T cycles
```

### wasm_memory_limit

Maximum heap size for the WASM module.

| Property | Value |
|----------|-------|
| Type | Integer |
| Unit | Bytes |
| Default | Platform default |

```yaml
settings:
  wasm_memory_limit: 1073741824  # 1GB
```

### wasm_memory_threshold

Memory threshold that triggers low-memory callbacks.

| Property | Value |
|----------|-------|
| Type | Integer |
| Unit | Bytes |
| Default | None |

```yaml
settings:
  wasm_memory_threshold: 536870912  # 512MB
```

### log_visibility

Controls who can read canister logs.

| Property | Value |
|----------|-------|
| Type | String or Object |
| Values | `controllers`, `public`, or `allowed_viewers` object |
| Default | `controllers` |

```yaml
# Only controllers can view logs (default)
settings:
  log_visibility: controllers

# Anyone can view logs
settings:
  log_visibility: public

# Specific principals can view logs
settings:
  log_visibility:
    allowed_viewers:
      - "aaaaa-aa"
      - "2vxsx-fae"
```

### environment_variables

Runtime environment variables accessible to the canister.

| Property | Value |
|----------|-------|
| Type | Object (string keys, string values) |
| Default | None |

```yaml
settings:
  environment_variables:
    API_URL: "https://api.example.com"
    DEBUG: "false"
    FEATURE_FLAGS: "advanced=true"
```

Environment variables allow the same WASM to run with different configurations.

## Full Example

```yaml
canisters:
  - name: backend
    build:
      steps:
        - type: script
          commands:
            - cargo build --target wasm32-unknown-unknown --release
            - cp target/wasm32-unknown-unknown/release/backend.wasm "$ICP_WASM_OUTPUT_PATH"
    settings:
      compute_allocation: 5
      memory_allocation: 2147483648       # 2GB
      freezing_threshold: 2592000         # 30 days
      reserved_cycles_limit: 5000000000000
      wasm_memory_limit: 1073741824       # 1GB
      wasm_memory_threshold: 536870912    # 512MB
      log_visibility: controllers
      environment_variables:
        ENV: "production"
        API_BASE_URL: "https://api.example.com"
```

## Environment Overrides

Override settings per environment:

```yaml
canisters:
  - name: backend
    settings:
      compute_allocation: 1  # Default

environments:
  - name: production
    network: mainnet
    canisters: [backend]
    settings:
      backend:
        compute_allocation: 20              # Production override
        freezing_threshold: 7776000         # 90 days
        environment_variables:
          ENV: "production"
```

## CLI Commands

View settings:

```bash
icp canister settings show my-canister
```

Update settings:

```bash
icp canister settings update my-canister --compute-allocation 10
```

Sync settings from configuration:

```bash
icp canister settings sync my-canister
```

## See Also

- [Configuration Reference](configuration.md) — Full icp.yaml schema
- [Managing Environments](../guides/managing-environments.md) — Environment-specific settings
- [CLI Reference](cli.md) — `canister settings` commands
