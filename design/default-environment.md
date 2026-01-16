# Default Environments for Networks

## TL;DR

To reduce cognitive burden for simple projects, each network will have an implicit default environment. Commands like `icp deploy --network local` will automatically target the default environment without requiring explicit `--environment` flags. Users only need to define custom environments when deploying multiple instances to the same network.

## Problem

The environment abstraction enables deploying multiple canister instances to a single network, but adds unnecessary complexity for common use cases where users only need one instance per network. Currently, users must understand and specify environments even for simple deployments.

## Proposed Solution

### Default Environment Behavior

Every network automatically has a corresponding default environment. When commands require an environment target but only `--network` is specified, the CLI uses the default environment for that network.

**Examples:**
```bash
# No flags: targets default environment of 'local' network (which is available by default)
icp deploy

# Explicitly specify network: targets default environment of 'mainnet' network
icp deploy --network mainnet

# Custom network: targets default environment of 'testnet' network
icp deploy --network testnet
```

### Custom Environments

When users need multiple instances on one network, they explicitly define custom environments in manifest files and specify both flags:

```bash
# Deploy to a custom 'staging' environment on mainnet
icp deploy --network mainnet --environment staging
```

**Configuration example:**
```yaml
networks:
  - name: testnet
    mode: connected
    gateway:
      url: https://testnet.example.com

environments:
  - name: staging
    network: mainnet
    canisters: [my-canister]
    # ... environment-specific settings
  
  - name: preview
    network: mainnet
    canisters: [my-canister]
    # ... environment-specific settings
```

**Note:** In simple projects, both `networks` and `environments` sections can be omitted entirely. The `local` and `mainnet` networks are available by default.

## Behavioral Changes

### Before
- Users must always understand the environment concept
- Commands require explicit `--environment` flag or default behavior is unclear
- Simple projects have unnecessary configuration overhead

### After
- **Simplest case:** `icp deploy` → uses default environment of `local` network
- **Simple case:** `icp deploy --network <name>` → uses default environment of specified network
- **Advanced case:** `icp deploy --network <name> --environment <env>` → uses custom environment
- `local` and `mainnet` networks available by default
- Environment concept only surfaces when actually needed
- Default environments are implicit and require no configuration

## Benefits

1. **Lower barrier to entry**: New users don't need to learn environments immediately
2. **Simpler mental model**: For basic use cases, "network" is sufficient
4. **Progressive disclosure**: Complexity only introduced when needed
