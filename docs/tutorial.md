# Tutorial

Deploy your first canister on the Internet Computer.

> **What is a canister?** A canister is your application running on the Internet Computer — it combines code and persistent state, with no servers to manage.

## Prerequisites

Follow the **[Installation Guide](guides/installation.md)** to install:
- icp-cli
- Language toolchains (Rust or Motoko)
- ic-wasm (required when using templates or recipes)

Verify installation:

```bash
icp --version
```

## Create a Project

```bash
icp new my-project
```

Select a template when prompted:
- **motoko** — Single Motoko canister (recommended for this tutorial)
- **rust** — Single Rust canister

*Choose the template matching the language you installed. Both work identically for this tutorial.*

> **Note:** The `hello-world` template creates a full-stack app with frontend and backend. It's great for building web apps, but adds complexity for a first deployment.

Enter the project directory:

```bash
cd my-project
```

Your project contains:
- `icp.yaml` — Project configuration
- `src/` — Source code
- `README.md` — Project-specific instructions

## Start the Local Network

```bash
icp network start -d
```

The `-d` flag runs the network in the background (detached) so you can continue using your terminal.

Verify the network is running:

```bash
icp network status
```

## Deploy

```bash
icp deploy
```

This single command:
1. **Builds** your source code into WebAssembly (WASM)
2. **Creates** a canister on the local network
3. **Installs** your WASM code into the canister

**Tip:** You can also run `icp build` separately if you want to verify compilation before deploying.

## Interact with Your Canister

List your deployed canister:

```bash
icp canister list
```

You should see one canister listed. Call its `greet` method using that name:

```bash
icp canister call <canister-name> greet
```

When you omit the argument, icp-cli prompts you to enter it interactively — just type `World` when asked.

You should see: `("Hello, World!")`

> **Tip:** You can also pass arguments directly using [Candid](https://docs.internetcomputer.org/building-apps/interact-with-canisters/candid/candid-concepts) format: `icp canister call <canister-name> greet '("World")'`

## Stop the Network

When you're done:

```bash
icp network stop
```

## Troubleshooting

**Something not working?** Check the [Installation Guide](guides/installation.md) troubleshooting section or run `icp network status` to verify your network is running.

## Next Steps

You've deployed your first canister! Continue your journey:

- [Local Development](guides/local-development.md) — Learn the day-to-day development workflow
- [Deploying to Mainnet](guides/deploying-to-mainnet.md) — Go live on the Internet Computer
- [Core Concepts](concepts/project-model.md) — Understand how icp-cli works (optional deep dive)

[Browse all documentation →](index.md)
