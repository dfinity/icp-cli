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
| Export identity | `dfx identity export my_id` | `icp identity export my_id` |
| Rename identity | `dfx identity rename old_id new_id` | `icp identity rename old_id new_id` |
| Delete identity | `dfx identity remove my_id` | `icp identity delete my_id` |
| Get account ID | `dfx ledger account-id` | `icp identity account-id` |

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

**icp.yaml:**
```yaml
canisters:
  - name: backend
    recipe:
      type: "@dfinity/motoko"
      configuration:
        main: src/backend/main.mo
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

**icp.yaml:**
```yaml
canisters:
  - name: frontend
    recipe:
      type: "@dfinity/asset-canister"
      configuration:
        dir: dist
```

**Note:** dfx automatically builds frontend assets by looking for `package.json` and running `npm run build`. With icp-cli, you need to specify build commands explicitly if your assets need to be built:

```yaml
canisters:
  - name: frontend
    recipe:
      type: "@dfinity/asset-canister"
      configuration:
        dir: dist
        build:
          - npm install
          - npm run build
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
      type: "@dfinity/asset-canister"
      configuration:
        dir: dist
        build:
          - npm install
          - npm run build

  - name: backend
    recipe:
      type: "@dfinity/rust"
      configuration:
        package: backend
```

**Key differences:**
- icp-cli doesn't have explicit dependencies between canisters (dfx's `dependencies` field)
- Frontend build commands must be specified explicitly in icp-cli
- Deploy order is determined automatically or you can deploy specific canisters

### Network Configuration

**Remote network example:**

**dfx.json:**
```json
{
  "networks": {
    "staging": {
      "providers": ["https://icp-api.io"],
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
    url: https://icp-api.io

environments:
  - name: staging
    network: staging
    canisters: [frontend, backend]
```

**Testnet with root key:**

**dfx.json:**
```json
{
  "networks": {
    "testnet": {
      "providers": ["https://testnet.example.com"],
      "type": "persistent"
    }
  }
}
```

**icp.yaml:**
```yaml
networks:
  - name: testnet
    mode: connected
    url: https://testnet.example.com
    root-key: 308182301d060d2b0601040182dc7c05030102...  # Hex-encoded root key
```

**Local network with custom bind address:**

**dfx.json:**
```json
{
  "networks": {
    "local": {
      "bind": "127.0.0.1:4943",
      "type": "ephemeral"
    }
  }
}
```

**icp.yaml:**
```yaml
networks:
  - name: local
    mode: managed
    gateway:
      host: 127.0.0.1
      port: 4943
```

**Key differences:**
- dfx's `"type": "persistent"` maps to icp-cli's `mode: connected` (external networks)
- dfx's `"type": "ephemeral"` maps to icp-cli's `mode: managed` (local networks that icp-cli controls)
- dfx's `"providers"` array (which can list multiple URLs for redundancy) becomes a single `url` field in icp-cli
- dfx's `"bind"` address for local networks maps to icp-cli's `gateway.host` and `gateway.port`
- **Root key handling**: dfx automatically fetches the root key from non-mainnet networks at runtime. icp-cli requires you to specify the `root-key` explicitly in the configuration for testnets (connected networks). For local managed networks, icp-cli retrieves the root key from the network launcher. The root key is the public key used to verify responses from the network. Explicit configuration ensures the root key comes from a trusted source rather than the network itself.

**Note:** icp-cli uses `https://icp-api.io` as the default IC mainnet URL, while dfx currently uses `https://icp0.io`. Both URLs point to the same IC mainnet, but `https://icp-api.io` is the recommended API gateway. The implicit `ic` network in icp-cli is configured with `https://icp-api.io`.

## Features Not in icp-cli

Some dfx features work differently or aren't directly available:

| dfx Feature | icp-cli Equivalent |
|-------------|-------------------|
| `dfx.json` defaults | Use recipes or explicit configuration |
| Canister dependencies | Use bindings compatible with Canister Environment Variables |
| `dfx generate` | Use language-specific tooling |
| `dfx ledger` | `icp token` and `icp cycles` commands |
| `dfx wallet` | Use `icp cycles balance` and `icp cycles transfer` |
| `dfx upgrade` | Reinstall icp-cli |

## Migrating Identities

dfx identities can be imported into icp-cli. Both tools use compatible key formats and support the same storage modes.

### Understanding Identity Storage

Both dfx and icp-cli support three storage modes:

- **Keyring** (default): Stores private keys in your system keychain/keyring
- **Password-protected**: Encrypts keys with a password in a file
- **Plaintext**: Stores unencrypted keys in a file (not recommended except for CI/CD)

**Default behavior:** Both tools try to use the system keyring first. If unavailable, dfx falls back to password-protected files.

### Identity Storage Locations

| Tool | Identity Directory | Structure |
|------|-------------------|-----------|
| **dfx** | `~/.config/dfx/identity/` | Per-identity subdirectories:<br>`<name>/identity.json` (metadata)<br>`<name>/identity.pem` (key, if not in keyring) |
| **icp-cli** | **macOS:** `~/Library/Application Support/org.dfinity.icp-cli/identity/`<br>**Linux:** `~/.local/share/icp-cli/identity/`<br>**Windows:** `%APPDATA%\icp-cli\data\identity\` | Centralized files:<br>`identity_list.json` (all identities)<br>`identity_defaults.json` (default selection)<br>`keys/<name>.pem` (keys, if not in keyring) |

**Private key storage (both tools):** System keyring (default), or encrypted/plaintext PEM files

**Note:** dfx and icp-cli use different service names in the system keyring (`internet_computer_identities` vs `icp-cli`), so identities must be explicitly migrated using the import/export process described below.

### Checking Your dfx Identity Storage Mode

To see how your dfx identity is stored:

```bash
cat ~/.config/dfx/identity/<name>/identity.json
```

Look for:
- `"keyring_identity_suffix": "<name>"` → Stored in system keyring
- `"encryption": {...}` → Password-protected file
- No `identity.json` or neither field present → Plaintext file

### Import dfx Identities

The import process depends on your dfx identity's storage mode.

#### For Keyring or Password-Protected Identities

Export from dfx first (this works for both storage types):

```bash
# Export from dfx (will prompt for password if encrypted)
dfx identity export my-identity > /tmp/my-identity.pem

# Import to icp-cli (uses keyring by default)
icp identity import my-identity --from-pem /tmp/my-identity.pem

# Clean up temporary file
rm /tmp/my-identity.pem

# Verify the principal matches
dfx identity get-principal --identity my-identity
icp identity principal --identity my-identity
```

Both commands should display the same principal.

#### For Plaintext Identities

If your dfx identity is stored as plaintext (has `identity.pem` file with no encryption):

```bash
# Direct import from dfx location
icp identity import my-identity \
  --from-pem ~/.config/dfx/identity/my-identity/identity.pem

# By default, icp-cli will store securely in keyring
# To keep as plaintext (not recommended):
icp identity import my-identity \
  --from-pem ~/.config/dfx/identity/my-identity/identity.pem \
  --storage plaintext
```

### Choosing Storage Mode in icp-cli

When importing, you can specify how icp-cli should store the private key:

```bash
# System keyring (default, recommended)
icp identity import my-id --from-pem key.pem --storage keyring

# Password-protected file
icp identity import my-id --from-pem key.pem --storage password

# Plaintext file (not recommended for production)
icp identity import my-id --from-pem key.pem --storage plaintext
```

If keyring is unavailable, icp-cli will prompt for a password to use password-protected storage.

### Migrate All Identities

To migrate all dfx identities at once:

```bash
# Export and import each identity
for id in $(dfx identity list | grep -v "^anonymous"); do
  echo "Migrating $id..."

  # Export from dfx (handles all storage types)
  dfx identity export "$id" > "/tmp/${id}.pem"

  # Import to icp-cli (uses keyring by default)
  icp identity import "$id" --from-pem "/tmp/${id}.pem"

  # Clean up
  rm "/tmp/${id}.pem"

  # Verify principals match
  echo "  dfx principal:     $(dfx identity get-principal --identity "$id")"
  echo "  icp-cli principal: $(icp identity principal --identity "$id")"
  echo ""
done

# List all imported identities
icp identity list
```

**Note:** This script copies identities to icp-cli without removing them from dfx. Your original dfx identities remain intact and both tools can be used side-by-side. The script will prompt for passwords if any dfx identities are password-protected or stored in keyring.

### Setting the Default Identity

After importing, set your default identity:

```bash
icp identity default my-identity
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

**icp-cli uses different storage paths based on network type:**
- **Connected networks (ic, mainnet):** `.icp/data/mappings/<environment>.ids.json`
- **Managed networks (local):** `.icp/cache/mappings/<environment>.ids.json`

For the ic environment, create `.icp/data/mappings/ic.ids.json`:

```json
{
  "frontend": "xxxxx-xxxxx-xxxxx-xxxxx-cai",
  "backend": "yyyyy-yyyyy-yyyyy-yyyyy-cai"
}
```

Get the canister IDs from your dfx project:

```bash
# dfx stores IDs in different locations depending on network type:
# - Persistent networks: canister_ids.json (project root)
# - Ephemeral networks: .dfx/<network>/canister_ids.json

# For mainnet/ic network:
dfx canister id frontend --network ic
dfx canister id backend --network ic
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

During migration, you can use both tools side-by-side with some considerations:

**What works side-by-side:**
- ✅ **Configuration files**: dfx uses `dfx.json`, icp-cli uses `icp.yaml` (no conflicts)
- ✅ **Identities**: Both store identities separately (dfx uses `internet_computer_identities` keyring service, icp-cli uses `icp-cli`), so they don't interfere with each other
- ✅ **Canister IDs**: Stored in different locations (`.dfx/` vs `.icp/`), no conflicts
- ✅ **Remote networks**: Both can deploy to IC mainnet independently

**Potential conflicts:**
- ⚠️ **Local networks**: Both default to `localhost:8000` for local development networks
  - **If running both local networks simultaneously**, they will conflict on port 8000
  - **Solution**: Configure icp-cli to use a different port by overriding the `local` network:
    ```yaml
    # icp.yaml
    networks:
      - name: local
        mode: managed
        gateway:
          port: 8001  # Use different port from dfx
    ```
  - Or stop dfx's local network before starting icp-cli's: `dfx stop` then `icp network start`

This allows gradual migration without disrupting existing workflows, as long as you manage local network ports.

## Getting Help

- [Tutorial](../tutorial.md) — Quick start guide
- [Concepts](../concepts/index.md) — Understand the icp-cli model
- [Configuration Reference](../reference/configuration.md) — Full icp.yaml documentation
