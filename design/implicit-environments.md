# Implicit Environments for Networks

## TL;DR

Establish a consistent rule: every network automatically has a corresponding implicit environment with the same name. This proposal renames `mainnet` to `ic` and applies the rule uniformly to both built-in networks (`local` and `ic`) and any user-defined networks.

## Problem

Currently, `icp` has `local` and `mainnet` networks defined implicitly, but only `local` has an implicit environment. Deploying to mainnet requires explicitly defining an environment pointing to `mainnet` (`icp deploy -e some_env_name`), creating inconsistency and unnecessary configuration overhead.

Adding an `ic` environment pointing to `mainnet` was considered but rejected—it would scatter implicit definitions without establishing a clear governing principle.

## Proposed Solution

### Implicit Environments

Every network automatically has a corresponding implicit environment with the same name. This applies to:
- Built-in networks: `local` network → `local` environment; `ic` network → `ic` environment
- User-defined networks: `canary` network → `canary` environment

These implicit environments require no configuration and are available immediately.

**Examples:**
```bash
# No flags: targets default 'local' environment (implicitly maps to 'local' network)
icp deploy

# Target 'ic' environment (implicitly maps to 'ic' network)
icp deploy -e ic

# Target custom network's implicit environment
icp deploy -e canary
```

### Network Renaming

The IC mainnet is renamed from `mainnet` to `ic` for clarity and brevity.

### Command Interface

Commands take either `-e/--environment` or `-n/--network` flags depending on what they operate on:

**Environment-based commands** (operate on deployed instances):
```bash
icp deploy -e ic
icp canister install -e local
```

**Network-based commands** (operate on network infrastructure):
```bash
icp network status -n ic
icp network start -n local
```

Environment names serve as unique identifiers throughout the project.

### Zero Configuration Projects

In projects without any explicit networks or environments defined in the manifest, the built-in `local` and `ic` networks and their implicit environments are immediately available:

**Deployment commands** (operate on environments):

```bash
# Deploy to local environment (default)
icp deploy

# Deploy to ic environment (mainnet)
icp deploy -e ic
```

**Network commands** (operate on networks):

```bash
# Check local network status (default)
icp network status

# Check ic network status (mainnet)
icp network status -n ic
```

No manifest configuration is required for these basic operations.

### Custom Environments

When users need multiple instances on one network, they define custom environments:

**Configuration example:**

```yaml
networks:
  - name: canary
    mode: connected
    gateway:
      url: https://canary.example.com

environments:
  # Custom environment on 'ic' network
  - name: staging
    network: ic
    canisters: [my-canister]
    # ... environment-specific settings
  
  # Customize the implicit 'local' environment
  # Note: 'network' field is omitted since the name matches the network
  - name: local
    canisters: [my-canister]
    settings:
      my-canister:
        memory_allocation: 100
```

**Commands based on the above configuration:**

```bash
# Deploy to custom 'staging' environment on ic network
icp deploy -e staging

# Deploy to customized implicit 'local' environment
icp deploy -e local
# or simply
icp deploy

# Deploy to implicit 'canary' environment (from custom canary network)
icp deploy -e canary

# Check status of custom canary network
icp network status -n canary
```

## Implementation Notes

- Implicit environment naming: network `<name>` → environment `<name>`
- Built-in networks: `local` and `ic` (formerly `mainnet`)
- Environment names must be unique across the project
- When customizing an implicit environment (name = network name), the `network` field should be omitted from manifest
- Default target (no flags): `local` environment
- No `--local` and `--ic` flags. Use `-n local`, `-e local`, `-n ic`, `-e ic` instead.

## Benefits

1. **Minimal changes**: Preserves existing manifest structure and behavior
2. **Lower barrier to entry**: New users work with implicit environments
3. **Simpler mental model**: One-to-one network-environment mapping by default
4. **Progressive disclosure**: Custom environments only needed for multiple instances
5. **Clearer semantics**: IC mainnet is simply "ic" for both the network and the environment
