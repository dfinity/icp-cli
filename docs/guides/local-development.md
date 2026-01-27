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

## Next Steps

- [Deploying to Mainnet](deploying-to-mainnet.md) — Go live with your canisters

[Browse all documentation →](../index.md)
