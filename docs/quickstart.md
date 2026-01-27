# Quickstart

Deploy a full-stack app to a local network in under 5 minutes.

## Prerequisites

Complete the [Installation Guide](guides/installation.md) first.

> **Tip:** An npm package for even simpler installation is coming soon.

## Steps

```bash
# 1. Create a new project with Motoko backend + React frontend
icp new my-project --subfolder hello-world \
  --define backend_type=motoko \
  --define frontend_type=react \
  --define network_type=Default && cd my-project

# 2. Start a local network (runs in background)
icp network start -d

# 3. Build and deploy
icp deploy

# 4. Call your backend canister
icp canister call backend greet '("World")'

# 5. Stop the local network when done
icp network stop
```

You should see `("Hello, World!")` — and after deploying, open the **frontend URL** shown in the output to see your app.

## What's next?

- [Tutorial](tutorial.md) — Understand each step in detail
- [Configuration Reference](reference/configuration.md) — Customize your project
- [Deploy to Mainnet](guides/deploying-to-mainnet.md) — Go live on the Internet Computer
