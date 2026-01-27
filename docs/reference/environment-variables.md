# Environment Variables Reference

Environment variables used by icp-cli.

## Build Script Variables

During `script` build steps, icp-cli sets the following environment variable:

### `ICP_WASM_OUTPUT_PATH`

A temporary file path where your build script must place the compiled WASM file.

icp-cli creates a temporary directory before running your build script and sets `ICP_WASM_OUTPUT_PATH` to a file path within it (e.g., `/tmp/abc123/out.wasm`). Your script must copy or write the final WASM to this location. After your script completes, icp-cli reads the WASM from this path and stores it for deployment.

**Example:**
```yaml
build:
  steps:
    - type: script
      commands:
        - cargo build --target wasm32-unknown-unknown --release
        - cp target/wasm32-unknown-unknown/release/my_canister.wasm "$ICP_WASM_OUTPUT_PATH"
```

The script also runs with the **canister directory as the current working directory**, so relative paths in your build commands resolve from there.

## CLI Configuration Variables

### `ICP_ENVIRONMENT`

Sets the default environment when no `-e/--environment` flag is provided.

| Default | `local` |
|---------|---------|

```bash
export ICP_ENVIRONMENT=staging
icp deploy  # Deploys to staging environment
```

This is equivalent to passing `-e staging` to commands that accept an environment flag. The explicit `-e` flag takes precedence over this variable.

### `ICP_NETWORK`

Sets the default network when no `-n/--network` flag is provided.

| Default | `local` |
|---------|---------|

```bash
export ICP_NETWORK=ic
icp token balance  # Checks balance on IC mainnet
```

This is equivalent to passing `-n ic` to commands that accept a network flag. The explicit `-n` flag takes precedence over this variable.

### `ICP_HOME`

Overrides the default location for global icp-cli data (identities, package cache).

By default, icp-cli stores global data in platform-standard directories:

| Platform | Default Location |
|----------|------------------|
| macOS | `~/Library/Application Support/org.dfinity.icp-cli/` |
| Linux | `~/.local/share/icp-cli/` |
| Windows | `%APPDATA%\icp-cli\data\` |

When `ICP_HOME` is set, all global data is stored in that directory instead:

```bash
export ICP_HOME=~/.icp

# Identities will be stored in ~/.icp/identity/
# Package cache will be stored in ~/.icp/pkg/
```

**Use cases:**
- Keep icp-cli data in a specific location
- Share identities across machines via a synced folder
- Isolate icp-cli data for testing

### `ICP_CLI_NETWORK_LAUNCHER_PATH`

Path to a custom network launcher binary.

By default, icp-cli automatically downloads the network launcher on first use. Set this variable to use a specific binary instead:

```bash
export ICP_CLI_NETWORK_LAUNCHER_PATH=/path/to/icp-cli-network-launcher
```

**Use cases:**
- Air-gapped or offline environments where auto-download isn't possible
- Testing a custom or development version of the launcher
- CI environments where you pre-download dependencies

Download the launcher manually from [icp-cli-network-launcher releases](https://github.com/dfinity/icp-cli-network-launcher/releases).

## Windows-Specific Variables

### `ICP_CLI_BASH_PATH`

Path to the bash executable on Windows.

icp-cli uses bash to run build scripts. On Windows, it searches for bash in common locations (Git Bash, MSYS2). If bash is not found automatically, set this variable:

```powershell
$env:ICP_CLI_BASH_PATH = "C:\Program Files\Git\bin\bash.exe"
```

**Common bash locations on Windows:**
- Git Bash: `C:\Program Files\Git\bin\bash.exe`
- MSYS2: `C:\msys64\usr\bin\bash.exe`

## See Also

- [Managing Identities](../guides/managing-identities.md) — Identity storage paths and directory contents
- [Project Model](../concepts/project-model.md) — Project directory structure (`.icp/`) and what's safe to delete
