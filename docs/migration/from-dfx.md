# Migrating from dfx

This guide helps developers familiar with dfx transition to icp-cli.

## Key Differences

### Configuration Format

| Aspect | dfx | icp-cli |
|--------|-----|---------|
| Config file | `dfx.json` | `icp.yaml` |
| Format | JSON | YAML |
| Canisters | Object with canister names as keys | Array of canister definitions |

### Deployment Model

**dfx** deploys to networks directly:
```bash
dfx deploy --network ic
```

**icp-cli** deploys to environments (which reference networks):
```bash
icp deploy --environment production
# or shorthand for mainnet:
icp deploy --ic
```

Environments add a layer of abstraction, allowing different settings for the same network.

### Recipe System

icp-cli introduces recipes — reusable build templates. Instead of dfx's built-in canister types, you reference recipes:

```yaml
# dfx.json style (not supported)
"my_canister": {
  "type": "rust",
  "package": "my_canister"
}

# icp-cli style
canisters:
  - name: my_canister
    recipe:
      type: "@dfinity/rust"
      configuration:
        package: my_canister
```

### Build Process

dfx has built-in build logic. icp-cli delegates to recipes or explicit build steps:

```yaml
canisters:
  - name: backend
    build:
      steps:
        - type: script
          commands:
            - cargo build --target wasm32-unknown-unknown --release
            - cp target/wasm32-unknown-unknown/release/backend.wasm "$ICP_WASM_OUTPUT_PATH"
```

## Command Mapping

| Task | dfx | icp-cli |
|------|-----|---------|
| Create project | `dfx new my_project` | `icp new my_project` |
| Start local network | `dfx start --background` | `icp network start -d` |
| Stop local network | `dfx stop` | `icp network stop` |
| Build canister | `dfx build my_canister` | `icp build my_canister` |
| Deploy all | `dfx deploy` | `icp deploy` |
| Deploy to mainnet | `dfx deploy --network ic` | `icp deploy --ic` |
| Call canister | `dfx canister call my_canister method '(args)'` | `icp canister call my_canister method '(args)'` |
| List canisters | `dfx canister id my_canister` | `icp canister list` |
| Canister status | `dfx canister status my_canister` | `icp canister status my_canister` |
| Create identity | `dfx identity new my_id` | `icp identity new my_id` |
| Use identity | `dfx identity use my_id` | `icp identity default my_id` |
| Show principal | `dfx identity get-principal` | `icp identity principal` |

## Converting dfx.json to icp.yaml

### Basic Rust Canister

**dfx.json:**
```json
{
  "canisters": {
    "backend": {
      "type": "rust",
      "package": "backend",
      "candid": "src/backend/backend.did"
    }
  }
}
```

**icp.yaml:**
```yaml
canisters:
  - name: backend
    recipe:
      type: "@dfinity/rust"
      configuration:
        package: backend
```

### Basic Motoko Canister

**dfx.json:**
```json
{
  "canisters": {
    "backend": {
      "type": "motoko",
      "main": "src/backend/main.mo"
    }
  }
}
```

**icp.yaml:**
```yaml
canisters:
  - name: backend
    recipe:
      type: "@dfinity/motoko"
      configuration:
        entry: src/backend/main.mo
```

### Asset Canister

**dfx.json:**
```json
{
  "canisters": {
    "frontend": {
      "type": "assets",
      "source": ["dist"]
    }
  }
}
```

**icp.yaml:**
```yaml
canisters:
  - name: frontend
    recipe:
      type: "@dfinity/assets"
      configuration:
        source: dist
```

### Multi-Canister Project

**dfx.json:**
```json
{
  "canisters": {
    "frontend": {
      "type": "assets",
      "source": ["dist"],
      "dependencies": ["backend"]
    },
    "backend": {
      "type": "rust",
      "package": "backend"
    }
  }
}
```

**icp.yaml:**
```yaml
canisters:
  - name: frontend
    recipe:
      type: "@dfinity/assets"
      configuration:
        source: dist

  - name: backend
    recipe:
      type: "@dfinity/rust"
      configuration:
        package: backend
```

Note: icp-cli doesn't have explicit dependencies between canisters. Deploy order is determined automatically or you can deploy specific canisters.

### Custom Build Commands

**dfx.json:**
```json
{
  "canisters": {
    "custom": {
      "type": "custom",
      "build": "make build",
      "wasm": "build/custom.wasm",
      "candid": "custom.did"
    }
  }
}
```

**icp.yaml:**
```yaml
canisters:
  - name: custom
    build:
      steps:
        - type: script
          commands:
            - make build
            - cp build/custom.wasm "$ICP_WASM_OUTPUT_PATH"
```

### Network Configuration

**dfx.json:**
```json
{
  "networks": {
    "staging": {
      "providers": ["https://ic0.app"],
      "type": "persistent"
    }
  }
}
```

**icp.yaml:**
```yaml
networks:
  - name: staging
    mode: connected
    url: https://ic0.app

environments:
  - name: staging
    network: staging
    canisters: [frontend, backend]
```

## Features Not in icp-cli

Some dfx features work differently or aren't directly available:

| dfx Feature | icp-cli Equivalent |
|-------------|-------------------|
| `dfx.json` defaults | Use recipes or explicit configuration |
| Canister dependencies | Deploy in desired order manually |
| `dfx generate` | Use language-specific tooling |
| `dfx ledger` | `icp token` commands |
| `dfx wallet` | Cycles managed differently |
| `dfx upgrade` | Reinstall icp-cli |

## Migration Steps

1. **Create icp.yaml** in your project root

2. **Convert canister definitions** using the examples above

3. **Test locally:**
   ```bash
   icp network start -d
   icp build
   icp deploy
   ```

4. **Verify canister functionality:**
   ```bash
   icp canister call my_canister method '(args)'
   ```

5. **Set up environments** for staging/production if needed

6. **Update CI/CD** scripts to use icp-cli commands

## Getting Help

- [Tutorial](../tutorial.md) — Quick start guide
- [Concepts](../concepts/index.md) — Understand the icp-cli model
- [Configuration Reference](../reference/configuration.md) — Full icp.yaml documentation
