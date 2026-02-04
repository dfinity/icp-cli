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

## Canister Runtime Environment Variables

These variables are stored in canister settings and accessible to canister code at runtime. They are distinct from the build-time and CLI configuration variables above.

### Automatic Variables

#### `PUBLIC_CANISTER_ID:<canister-name>`

During deployment, icp-cli automatically creates environment variables containing the canister IDs of all canisters in the current environment.

| Property | Value |
|----------|-------|
| Format | `PUBLIC_CANISTER_ID:<name>` |
| Value | Canister principal (text) |
| When Set | Automatically during `icp deploy` |

**Example:**

For an environment with canisters `backend` and `frontend`:

```
PUBLIC_CANISTER_ID:backend  = bkyz2-fmaaa-aaaaa-qaaaq-cai
PUBLIC_CANISTER_ID:frontend = bd3sg-teaaa-aaaaa-qaaba-cai
```

**Purpose:** Enables canisters to discover other canisters in the same environment without hardcoding IDs. This is especially important for:

- Frontend canisters calling backend canisters
- Multi-canister architectures with service dependencies
- Environment-agnostic deployments (same code works in local, staging, production)

#### `IC_ROOT_KEY`

The asset canister automatically includes the network's root key in the `ic_env` cookie. This is **not** set by icp-cli during deployment — the asset canister provides it directly based on the network it's running on.

| Property | Value |
|----------|-------|
| Cookie key | `ic_root_key` (lowercase) |
| Cookie value | Hex-encoded root key |
| When Set | By the asset canister at request time |

**Purpose:** The root key is required by the IC agent to verify response signatures. By providing it via the cookie, frontends work consistently across local networks and mainnet without code changes.

**How frontends access these:** Use `@icp-sdk/core/agent/canister-env` to read the `ic_env` cookie. The library parses the hex-encoded root key and returns it as `Uint8Array`:

```typescript
import { getCanisterEnv } from "@icp-sdk/core/agent/canister-env";

interface CanisterEnv {
  "PUBLIC_CANISTER_ID:backend": string;
  IC_ROOT_KEY: Uint8Array;  // Converted from hex by the library
}

const env = getCanisterEnv<CanisterEnv>();
const backendId = env["PUBLIC_CANISTER_ID:backend"];
const rootKey = env.IC_ROOT_KEY;  // Ready for agent's rootKey option
```

See [Canister Discovery](../concepts/canister-discovery.md) for detailed usage patterns.

### Custom Variables

Define custom runtime variables in canister settings:

```yaml
canisters:
  - name: backend
    settings:
      environment_variables:
        API_ENDPOINT: "https://api.example.com"
        DEBUG: "false"
```

Override per environment:

```yaml
environments:
  - name: production
    network: ic
    canisters: [backend]
    settings:
      backend:
        environment_variables:
          API_ENDPOINT: "https://api.prod.example.com"
```

See [Canister Settings Reference](canister-settings.md#environment_variables) for full configuration options.

## See Also

- [Canister Discovery](../concepts/canister-discovery.md) — How canisters discover each other
- [Local Development](../guides/local-development.md#frontend-development) — Frontend development workflow
- [Canister Settings Reference](canister-settings.md) — Full settings documentation
- [Managing Identities](../guides/managing-identities.md) — Identity storage paths and directory contents
- [Project Model](../concepts/project-model.md) — Project directory structure (`.icp/`) and what's safe to delete
