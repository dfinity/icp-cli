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

Fixed memory reservation.

| Property | Value |
|----------|-------|
| Type | Integer or string with suffix |
| Unit | Bytes (accepts suffixes) |
| Default | Dynamic allocation |

```yaml
settings:
  memory_allocation: 4gib
```

Memory values accept suffixes: `kb` (1,000), `kib` (1,024), `mb` (1,000,000), `mib` (1,048,576), `gb` (1,000,000,000), `gib` (1,073,741,824). Decimals are supported (e.g. `2.5gib`). Raw byte counts are also accepted.

If not set, the canister uses dynamic memory allocation.

### freezing_threshold

Time before the canister freezes due to low cycles.

| Property | Value |
|----------|-------|
| Type | Integer or string with duration suffix |
| Unit | Seconds (accepts duration suffixes) |
| Default | 2,592,000 seconds (30 days) |

```yaml
settings:
  freezing_threshold: 90d
```

Duration values accept suffixes: `s` (seconds), `m` (minutes), `h` (hours), `d` (days), `w` (weeks). Underscores are supported in the numeric part (e.g. `2_592_000`). A bare number is treated as seconds. Raw second counts are also accepted for backwards compatibility.

The canister freezes if its cycles balance would be exhausted within this threshold.

### reserved_cycles_limit

Upper limit on cycles reserved for future resource payments. When a canister allocates new storage on a subnet above 750 GiB usage, cycles are moved from its main balance into a reserved balance to pre-pay for future storage costs. This setting caps that reserved balance — memory allocations that would push it above the limit will fail. Set to `0` to disable resource reservation entirely (prevents memory allocation on subnets above 750 GiB).

| Property | Value |
|----------|-------|
| Type | Integer or string with suffix |
| Unit | Cycles (accepts suffixes) |
| IC Default | 5,000,000,000,000 (5T) |

```yaml
settings:
  reserved_cycles_limit: 1t
```

Cycles values accept suffixes: `k` (thousand), `m` (million), `b` (billion), `t` (trillion). Decimals and underscores are supported (e.g. `1.5t`, `500_000`). Raw integers are also accepted.

### wasm_memory_limit

Maximum heap size for the WASM module.

| Property | Value |
|----------|-------|
| Type | Integer or string with suffix |
| Unit | Bytes (accepts suffixes) |
| Default | Platform default |

```yaml
settings:
  wasm_memory_limit: 1gib
```

### wasm_memory_threshold

Memory threshold that triggers low-memory callbacks.

| Property | Value |
|----------|-------|
| Type | Integer or string with suffix |
| Unit | Bytes (accepts suffixes) |
| Default | None |

```yaml
settings:
  wasm_memory_threshold: 512mib
```

### log_memory_limit

Maximum memory for storing canister logs. Oldest logs are purged when usage exceeds this limit.

| Property | Value |
|----------|-------|
| Type | Integer or string with suffix |
| Unit | Bytes (accepts suffixes) |
| Max | 2 MiB |
| Default | 4096 bytes |

```yaml
settings:
  log_memory_limit: 2mib
```

Memory values accept suffixes: `kb` (1,000), `kib` (1,024), `mb` (1,000,000), `mib` (1,048,576). Raw byte counts are also accepted.

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

### snapshot_visibility

Controls who can read the canister's snapshots — the `list_canister_snapshots`,
`read_canister_snapshot_metadata`, and `read_canister_snapshot_data` endpoints of
the management canister, which back `icp canister snapshot list` and
`icp canister snapshot download`.

| Property | Value |
|----------|-------|
| Type | String or Object |
| Values | `controllers`, `public`, or `allowed_viewers` object |
| Default | `controllers` |

```yaml
# Only controllers can read the snapshots (default)
settings:
  snapshot_visibility: controllers

# Anyone can read the snapshots
settings:
  snapshot_visibility: public

# Specific principals can read the snapshots, in addition to the controllers
settings:
  snapshot_visibility:
    allowed_viewers:
      - "aaaaa-aa"
      - "2vxsx-fae"
```

Reading a snapshot exposes the canister's full state — WASM module, WASM memory,
stable memory, and chunk store. Granting snapshot access is therefore closer to
granting a state dump than to granting the read-only report
[`status_visibility`](#status_visibility) covers.

Like [`log_visibility`](#log_visibility) and unlike
[`status_visibility`](#status_visibility), `controllers` here is exhaustive:
there are no always-allowed callers on top of it.

Taking, restoring, and deleting snapshots stays controller-only whatever this is
set to; the setting governs reading alone.

The replica accepts at most 10 principals in `allowed_viewers`.

### status_visibility

Controls who can read the canister's status through the management canister's
`canister_status` endpoint — the report `icp canister status` prints, covering
the running state, cycles balance, memory usage, and the settings themselves.

| Property | Value |
|----------|-------|
| Type | String or Object |
| Values | `controllers`, `public`, or `allowed_viewers` object |
| Default | `controllers` |

```yaml
# The canister's controllers can read the status (default)
settings:
  status_visibility: controllers

# Anyone can read the status
settings:
  status_visibility: public

# Specific principals can read the status, in addition to the controllers
settings:
  status_visibility:
    allowed_viewers:
      - "aaaaa-aa"
      - "2vxsx-fae"
```

Two callers are always allowed, whatever the setting says: the administrators of
the subnet the canister runs on, and the canister itself reading its own status.
Unlike [`log_visibility`](#log_visibility), which grants access to the
controllers and listed viewers alone, `controllers` here is a floor rather than
an exhaustive list.

A caller that is not allowed to read the status still sees the canister's
controllers and module hash, which the replica publishes in the state tree and
`icp canister status` falls back to. Granting status access does not grant any
control over the canister.

The replica accepts at most 10 principals in `allowed_viewers`.

### environment_variables

Runtime environment variables accessible to the canister.

| Property | Value |
|----------|-------|
| Type | Object (string keys; values are strings or `{ path: <file> }`) |
| Default | None |

```yaml
settings:
  environment_variables:
    API_URL: "https://api.example.com"
    DEBUG: "false"
    FEATURE_FLAGS: "advanced=true"
```

Environment variables allow the same WASM to run with different configurations.

#### Values from a file

A value can be read from a file instead of being written inline, which keeps
values you would rather not commit — or that another tool generates — out of the
manifest:

```yaml
settings:
  environment_variables:
    API_URL: "https://api.example.com"
    API_KEY:
      path: ./secrets/api-key
```

The path is relative to the canister's own directory — the directory holding its
`canister.yaml`, or the project directory for a canister declared inline in
`icp.yaml`. An [environment override](#environment-overrides) resolves against
that same directory, not against the manifest declaring the override, so a path
means the same thing wherever it is written. This matches how an `init_args`
override resolves its path.

Surrounding whitespace is trimmed off the file's contents, so a trailing newline
does not become part of the value.

The file is read when the project is loaded, so a missing or unreadable file
fails the command before anything is deployed. `icp project bundle` reads the file
and writes the value into the bundled manifest inline — the file itself does not
travel with the bundle, and a file outside the project is rejected rather than
bundled.

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
      memory_allocation: 2gib
      freezing_threshold: 30d
      reserved_cycles_limit: 5t
      wasm_memory_limit: 1gib
      wasm_memory_threshold: 512mib
      log_visibility: controllers
      status_visibility: controllers
      log_memory_limit: 2mib
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
        freezing_threshold: 90d
        environment_variables:
          ENV: "production"
          API_KEY:
            path: ./secrets/production-api-key
```

File references inside an override — `environment_variables` values,
`init_args`, and `upgrade_args` alike — resolve against the *referenced canister's* directory, not the
directory of the manifest declaring the override. For a canister that comes from a
[dependency](../concepts/project-dependencies.md), that is the dependency's own
directory.

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
