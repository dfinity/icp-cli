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

# or use the implicit ic environment:
icp deploy --environment ic
icp deploy -e ic
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

dfx has built-in build logic. icp-cli delegates to the appropriate toolchain as specified in the
build configuration or through the use of a recipe.

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

### Build parallelism

dfx requires users to specify the inter canister dependencies so it can build canisters in order.

icp-cli assumes users will use canister environment variables to connect canisters and builds all canisters in parallel.

### Local networks

| Operation | dfx | icp-cli |
|-----------|-----|---------|
| Launching a local network | Shared local network for all projects | Local network is local to the project |
| System canisters | Requires that you pass additional parameters to setup system canisters | Launches a network with system canisters and seeds accounts with ICP and Cycles |
| Tokens | User must mint tokens | Anonymous principal and local account are seeded with tokens |
| docker support | N/A | Supports launching a dockerized network |


## Command Mapping

| Task | dfx | icp-cli |
|------|-----|---------|
| Create project | `dfx new my_project` | `icp new my_project` |
| Start local network | `dfx start --background` | `icp network start -d` |
| Stop local network | `dfx stop` | `icp network stop` |
| Build canister | `dfx build my_canister` | `icp build my_canister` |
| Deploy all | `dfx deploy` | `icp deploy` |
| Deploy to mainnet | `dfx deploy --network ic` | `icp deploy -e ic` |
| Call canister | `dfx canister call my_canister method '(args)'` | `icp canister call my_canister method '(args)'` |
| Get canister ID | `dfx canister id my_canister` | `icp canister status my_canister --id-only` |
| List canisters | `dfx canister ls` | `icp canister list` |
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

**canister.yaml:**
```yaml
name: backend
recipe:
  type: "@dfinity/rust"
  configuration:
    package: backend
    candid: "src/backend/backend.did"
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

**canister.yaml:**
```yaml
name: backend
recipe:
  type: "@dfinity/motoko"
  configuration:
    entry: src/backend/main.mo
    candid: src/backend/candid.did
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

**canister.yaml:**
```yaml
name: frontend
recipe:
  type: "@dfinity/asset-canister"
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
| Canister dependencies | Use bindings compatible with Canister Environment Variables |
| `dfx generate` | Use language-specific tooling |
| `dfx ledger` | `icp token` commands |
| `dfx wallet` | Cycles managed differently |
| `dfx upgrade` | Reinstall icp-cli |

## Migrating Identities

dfx identities can be imported into icp-cli. Both tools use compatible key formats.

### Identity Storage Locations

| Tool | Location |
|------|----------|
| dfx | `~/.config/dfx/identity/<name>/identity.pem` |
| icp-cli | `~/.config/icp/identity/` |

### Import a dfx Identity

```bash
# Import an unencrypted dfx identity
icp identity import my-identity --from-pem ~/.config/dfx/identity/my-identity/identity.pem

# Verify the principal matches
dfx identity get-principal --identity my-identity
icp identity principal --identity my-identity
```

Both commands should display the same principal.

### Import an Encrypted dfx Identity

If your dfx identity is password-protected:

```bash
icp identity import my-identity \
  --from-pem ~/.config/dfx/identity/my-identity/identity.pem \
  --decryption-password-from-file password.txt
```

Or enter the password interactively when prompted.

### Migrate All Identities

To migrate all dfx identities:

```bash
# List dfx identities
ls ~/.config/dfx/identity/

# Import each one
for id in $(ls ~/.config/dfx/identity/); do
  if [ -f ~/.config/dfx/identity/$id/identity.pem ]; then
    echo "Importing $id..."
    icp identity import $id --from-pem ~/.config/dfx/identity/$id/identity.pem
  fi
done

# Verify
icp identity list
```

### Setting the Default Identity

After importing, set your default identity:

```bash
icp identity default my-identity
```

### Identity Storage Options

When importing, choose how icp-cli stores the key:

```bash
# System keyring (recommended, default)
icp identity import my-id --from-pem key.pem --storage keyring

# Password-protected file
icp identity import my-id --from-pem key.pem --storage password

# Plaintext file (not recommended for production)
icp identity import my-id --from-pem key.pem --storage plaintext
```

## Migration Checklist

A complete migration involves these steps:

### 1. Create icp.yaml

Create `icp.yaml` in your project root using the conversion examples above.

### 2. Migrate Identities

Import the identities you use for this project:

```bash
icp identity import deployer --from-pem ~/.config/dfx/identity/deployer/identity.pem
```

### 3. Test Locally

```bash
icp network start -d
icp build
icp deploy
icp canister call my-canister test_method '()'
```

### 4. Migrate Canister IDs (Optional)

If you have existing canisters on mainnet that you want to continue managing with icp-cli, create a mapping file to preserve their IDs.

Create `.icp/data/mappings/ic.ids.json`:

```json
{
  "frontend": "xxxxx-xxxxx-xxxxx-xxxxx-cai",
  "backend": "yyyyy-yyyyy-yyyyy-yyyyy-cai"
}
```

Get the canister IDs from your dfx project:

```bash
dfx canister --network ic id frontend
dfx canister --network ic id backend
```

### 5. Verify Mainnet Access

```bash
# Check you can reach IC mainnet
icp network ping ic

# Verify identity has correct principal
icp identity principal

# Check canister status (if you migrated IDs)
icp canister status my-canister -e ic
```

### 6. Update CI/CD

Replace dfx commands with icp-cli equivalents in your CI/CD scripts:

**Before (dfx):**
```yaml
steps:
  - run: dfx start --background
  - run: dfx deploy
  - run: dfx deploy --network ic
```

**After (icp-cli):**
```yaml
steps:
  - run: icp network start -d
  - run: icp deploy
  - run: icp deploy -e ic
```

### 7. Update Documentation

Update any project documentation that references dfx commands.

## Keeping Both Tools

During migration, you can use both tools side-by-side:

- dfx and icp-cli use separate configuration files (`dfx.json` vs `icp.yaml`)
- Identity files can be shared by importing into icp-cli
- Canister IDs are stored in different locations

This allows gradual migration without disrupting existing workflows.

## Getting Help

- [Tutorial](../tutorial.md) — Quick start guide
- [Concepts](../concepts/index.md) — Understand the icp-cli model
- [Configuration Reference](../reference/configuration.md) — Full icp.yaml documentation
