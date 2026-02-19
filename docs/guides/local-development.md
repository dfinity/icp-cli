# Local Development

This guide covers the day-to-day development workflow with icp-cli.

## The Development Cycle

Local development follows a simple loop:

```
Edit code → Build → Deploy → Test → Repeat
```

### Starting Your Session

Start the local network in the background:

```bash
icp network start -d
```

Verify it's running:

```bash
icp network ping
```

### Making Changes

After editing your source code, deploy the changes:

```bash
icp deploy
```

This rebuilds and redeploys all canisters. Deploy specific canisters:

```bash
icp deploy my-canister
```

**Tip:** `icp deploy` always builds first. If you want to verify compilation before deploying, run `icp build` separately.

### Testing Changes

Call methods on your canister:

```bash
icp canister call my-canister method_name '(arguments)'
```

Example:

```bash
icp canister call backend get_user '("alice")'
```

For read-only methods, use `--query` for faster uncertified responses:

```bash
icp canister call backend get_user '("alice")' --query
```

### Forwarding Cycles with the Proxy Canister

Managed networks include a proxy canister that forwards calls with cycles attached. This is useful for testing methods that require cycles:

```bash
icp canister call my-canister method '(args)' \
  --proxy $(icp network status --json | jq -r .proxy_canister_principal) \
  --cycles 1T
```

The proxy canister's principal is shown in `icp network status` output.

### Viewing Project State

List canisters configured in this environment (the `local` environment is the default, targeting your local network):

```bash
icp canister list
```

View the effective project configuration:

```bash
icp project show
```

## Working with Multiple Canisters

Deploy all canisters:

```bash
icp deploy
```

Deploy specific canisters:

```bash
icp deploy frontend
icp deploy backend
```

Build without deploying (for verification):

```bash
icp build           # Build all
icp build frontend  # Build specific canister
```

## Frontend Development

### Asset Canisters

Web frontends on the Internet Computer are served by **asset canisters** — pre-built canisters maintained by DFINITY that serve static files (HTML, JS, CSS, images) over HTTP.

The `@dfinity/asset-canister` recipe deploys this pre-built canister and syncs your frontend files to it:

```yaml
canisters:
  - name: frontend
    recipe:
      type: "@dfinity/asset-canister"
      configuration:
        dir: dist  # Your built frontend files
```

Deploy and access your frontend:

```bash
icp network start -d
icp deploy
```

Open your browser to `http://<frontend-canister-id>.localhost:8000/` (the canister ID is shown in the deploy output).

### Calling Backend Canisters

This section applies when your frontend needs to call backend canisters. If your frontend is purely static, you can skip this.

When a frontend calls a backend canister, it needs two things:

1. **The backend's canister ID** — to know which canister to call
2. **The network's root key** — to verify response signatures

Asset canisters solve this automatically via a cookie named `ic_env`:

1. During `icp deploy`, canister IDs are injected as `PUBLIC_CANISTER_ID:*` canister environment variables
2. The asset canister serves these variables plus the network's root key via the `ic_env` cookie
3. Your frontend reads the cookie using `@icp-sdk/core` to get canister IDs and root key

This works identically on local networks and mainnet — your frontend code doesn't need to change between environments.

See [Canister Discovery](../concepts/canister-discovery.md) for implementation details.

### Development Approaches

When developing a frontend that calls backend canisters, you have two options:

| Approach | Best for | Trade-offs |
|----------|----------|------------|
| **Deploy and access asset canister** | Testing production-like behavior | No hot reload; must redeploy on every change |
| **Use a local dev server** | Fast iteration during development | Requires manual configuration |

#### Option 1: Deploy and access the asset canister

Deploy all canisters and access the frontend through the asset canister:

```bash
icp deploy
```

Open `http://<frontend-canister-id>.localhost:8000/`

The asset canister automatically sets the `ic_env` cookie with canister IDs and the network's root key.

**Limitation:** No hot module replacement. You must run `icp deploy frontend` after every frontend change.

#### Option 2: Use a local dev server

For hot reloading, run a dev server (Vite, webpack, etc.) that serves your frontend locally. Since your dev server isn't the asset canister, you need to configure it to provide the `ic_env` cookie.

**Key insight:** You only need to deploy the backend canister — the frontend canister isn't needed since your dev server serves the frontend.

```bash
icp deploy backend  # Only deploy backend
npm run dev         # Start your dev server
```

### Configuring a Dev Server

When using a dev server, configure it to:

1. **Fetch canister IDs and root key** from the CLI at startup
2. **Set the `ic_env` cookie** with these values (mimics what asset canisters do)
3. **Proxy `/api` requests** to the target network

See the [frontend-environment-variables example](https://github.com/dfinity/icp-cli/tree/main/examples/icp-frontend-environment-variables) for a complete Vite configuration.

**Workflow:**

```bash
icp network start -d   # Start local network
icp deploy backend     # Deploy backend canister
npm run dev            # Start dev server (fetches IDs automatically)
```

**Important:** After `icp network stop` and restart, the dev server will automatically fetch new canister IDs on next startup.

### Example Projects

- **hello-world template** — The template from `icp new` shows the complete pattern for reading the `ic_env` cookie. This is the simplest starting point.
- **[frontend-environment-variables example](https://github.com/dfinity/icp-cli/tree/main/examples/icp-frontend-environment-variables)** — A detailed Vite setup showing dev server configuration: fetching canister IDs and root key via CLI, setting the `ic_env` cookie, and using `@icp-sdk/core` to parse environment variables.

## Resetting State

To start fresh with a clean network:

```bash
# Stop the current network
icp network stop

# Start a new network (previous state is discarded)
icp network start -d
```

Then redeploy your canisters:

```bash
icp deploy
```

## Network Management

Check network status:

```bash
icp network status
```

View network details as JSON:

```bash
icp network status --json
```

Example output for a local managed network:

```json
{
  "managed": true,
  "api_url": "http://localhost:8000",
  "gateway_url": "http://localhost:8000",
  "candid_ui_principal": "be2us-64aaa-aaaaa-qaabq-cai",
  "proxy_canister_principal": "bd3sg-teaaa-aaaaa-qaaba-cai",
  "root_key": "308182..."
}
```

| Field | Description |
|-------|-------------|
| `managed` | Whether icp-cli controls this network's lifecycle |
| `api_url` | Endpoint for canister calls |
| `gateway_url` | Endpoint for browser access to canisters |
| `candid_ui_principal` | Candid UI canister for testing (managed networks only) |
| `proxy_canister_principal` | Proxy canister for forwarding calls with cycles (managed networks only) |
| `root_key` | Network's root key for verifying responses |

For connected networks (like `ic`), `candid_ui_principal` and `proxy_canister_principal` are omitted.

Stop the network when done:

```bash
icp network stop
```

## Troubleshooting

**Build fails with "command not found"**

A required tool is missing. See the [Installation Guide](installation.md) for:
- **Rust toolchain** — If error mentions `cargo` or `rustc`
- **Motoko toolchain** — If error mentions `moc` or `mops`
- **ic-wasm** — If error mentions `ic-wasm`

**Network connection fails**

Check if the network is running:

```bash
icp network ping
```

If not responding, restart:

```bash
icp network stop
icp network start -d
```

**Deployment fails**

1. Verify the build succeeded: `icp build`
2. Check network health: `icp network ping`

**Frontend can't find canister IDs**

If using a dev server, ensure you've deployed the backend before starting:

```bash
icp deploy backend
npm run dev  # Start after deploy
```

If accessing the asset canister directly, check that you're using the correct URL format: `http://<canister-id>.localhost:8000/`

## Next Steps

- [Canister Discovery](../concepts/canister-discovery.md) — How canisters find each other
- [Deploying to Mainnet](deploying-to-mainnet.md) — Go live with your canisters

[Browse all documentation →](../index.md)
