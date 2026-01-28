# Build, Deploy, Sync

Canisters go through three distinct phases when moving from source code to running on the Internet Computer.

```
Source Code → [Build] → WASM → [Deploy] → Running Canister → [Sync] → Configured State
```

Each phase has a specific purpose:

| Phase | Purpose | Commands |
|-------|---------|----------|
| **Build** | Compile source to WASM | `icp build` or `icp deploy` |
| **Deploy** | Create canister and install WASM | `icp deploy` |
| **Sync** | Post-deployment configuration | `icp deploy` or `icp sync` |

**Note:** `icp deploy` runs all three phases in sequence. Use individual commands when you need more control.

## Build Phase

The build phase transforms your source code into WebAssembly (WASM) bytecode.

### What Happens

1. Build steps from your configuration execute in sequence
2. Each step can run commands, copy files, or process assets
3. The final output is a `.wasm` file ready for deployment

### Key Points

- icp-cli **delegates** compilation to your language toolchain (Cargo for rust, mops for Motoko, etc.)
- Build output should be **reproducible** — no environment specific values should be baked in.
- The toolchain decides whether rebuilding is necessary.
- As part of the build phase you might build assets to be synchronized to the canister after the WASM is installed. For example, bundled web assets to serve a frontend.

### Build Step Types

**Script** — Run shell commands:

```yaml
build:
  steps:
    - type: script
      commands:
        - cargo build --target wasm32-unknown-unknown --release
        - cp target/wasm32-unknown-unknown/release/my_canister.wasm "$ICP_WASM_OUTPUT_PATH"
```

**Pre-built** — Use existing WASM:

```yaml
build:
  steps:
    - type: pre-built
      path: dist/canister.wasm
      sha256: abc123...  # Optional integrity check
```

**Assets** — Bundle static files:

```yaml
build:
  steps:
    - type: script
      commands:
        - npm run build
```

### Environment Variables

Scripts have access to:

- `ICP_WASM_OUTPUT_PATH` — Where to place the final WASM

Scripts run with the canister directory as the current working directory.

See [Environment Variables Reference](../reference/environment-variables.md) for all available variables.

## Deploy Phase

The deploy phase creates or updates canisters on a network.

### First Deployment

When deploying a canister for the first time:

1. An empty canister is **created** on the network
2. The canister receives a unique **canister ID**
3. Initial **cycles** are allocated
4. Canister **settings** are applied (memory, compute allocation, etc.)
5. Your WASM code is **installed**

### Subsequent Deployments

When the canister already exists:

1. The existing canister is located by ID
2. New WASM code is **upgraded** (preserving stable memory)
3. Settings are updated if changed

## Sync Phase

The sync phase handles post-deployment operations that depend on the canister being deployed.

### Common Use Cases

- **Asset canisters** — Upload static files after the canister is running

### Asset Sync

For frontend canisters, sync uploads your built assets:

```yaml
sync:
  steps:
    - type: assets
      dir: dist
```

### When Sync Runs

- Automatically after `icp deploy`
- Manually with `icp sync`

Run sync without redeploying:

```bash
icp sync my-canister
```

## The Full Picture

### What `icp deploy` Does

The `icp deploy` command is a composite command that executes multiple steps in sequence:

1. **Build** — Compile all target canisters to WASM (always runs)
2. **Create** — Create canisters on the network (only for canisters that don't exist yet)
3. **Update Canister Environment Variables** — Apply the updated Canister Environment Variables. These include variables used by bindings allowing canister interactions.
4. **Update Settings** — Apply canister settings (controllers, memory allocation, compute allocation, etc.)
5. **Install** — Install WASM code into canisters (always runs)
6. **Sync** — Run post-deployment steps like asset uploads (only if sync steps are configured)

### Initial vs Follow-up Deployments

**First deployment:**
- All steps run
- New canisters are created on the network
- Settings are applied
- WASM code is installed (install mode)

**Subsequent deployments:**
- Skip the canister creation
- Settings and Environment Variables are applied if they've changed.
- WASM code is upgraded, preserving canister state

Unlike `icp canister create` (which prints "already exists" and exits), `icp deploy` silently skips creation for existing canisters and continues with the remaining steps.

### Install Modes

The `--mode` flag controls how WASM is installed:

```bash
# Auto (default) — install for new canisters, upgrade for existing
icp deploy

# Install — only works on empty canisters
icp deploy --mode install

# Upgrade — preserves state, runs upgrade hooks
icp deploy --mode upgrade

# Reinstall — clears all state (use with caution)
icp deploy --mode reinstall
```

### Equivalent Individual Commands

What `icp deploy` does can be broken down into:

```bash
icp build                         # 1. Build
icp canister create               # 2. Create (if needed)
# (canister env vars updated)     # 3. Set environment variables
# (canister settings updated)     # 4. Sync settings
icp canister install --mode auto  # 5. Install
icp sync                          # 6. Sync (if configured)
```

### Running Phases Separately

For more control, run phases individually:

```bash
# Build only — compile without deploying
icp build

# Sync only — re-upload assets without rebuilding or reinstalling
icp sync
```

**When to run separately:**

- `icp build` — Verify compilation succeeds before deploying
- `icp sync` — Update assets without redeploying code (faster iteration for frontends)

**Note:** `icp deploy` always builds first. There's no way to skip the build phase during deploy. The build phase relies on the underlying toolchain (Cargo, moc, etc.) handling incremental compilation.

## Next Steps

- [Local Development](../guides/local-development.md) — Apply this in practice

[Browse all documentation →](../index.md)
