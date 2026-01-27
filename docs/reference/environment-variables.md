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

### `ICP_HOME`

Overrides the default location for global icp-cli data (identities, package cache).

By default, icp-cli stores global data in platform-standard directories:

| Platform | Default Location |
|----------|------------------|
| macOS | `~/Library/Application Support/icp-cli/` |
| Linux | `~/.local/share/icp-cli/` |
| Windows | `%APPDATA%\icp-cli\` |

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

## See Also

- [Project Model](../concepts/project-model.md#generated-files) — Project directory structure (`.icp/`) and what's safe to delete
