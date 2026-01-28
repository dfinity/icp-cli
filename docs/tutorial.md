# Tutorial

This tutorial walks through deploying a full-stack app on the Internet Computer, explaining each step along the way.

> **Already did the Quickstart?** This tutorial covers the same steps with detailed explanations. The Quickstart used `--define` flags to skip the interactive prompts — here you'll see what those prompts are and what they mean.

## What is a Canister?

A **canister** is your application running on the Internet Computer. It combines code and persistent state into a single unit — no servers to manage, no databases to configure. Your code runs on a decentralized network and persists automatically.

In this tutorial, you'll deploy two canisters:
- A **backend** canister (Motoko) — your application logic
- A **frontend** canister (React) — your web UI, also served from the blockchain

## Prerequisites

Complete the **[Installation Guide](guides/installation.md)** first.

Verify icp-cli is installed:

```bash
icp --version
```

## Create a Project

```bash
icp new my-project
```

You'll see three prompts:

**1. Template selection** — Choose `hello-world` for a full-stack app with backend and frontend.

**2. Backend language** — Choose `motoko` (or `rust` if you prefer).

**3. Network type** — Choose `Default` for native local networks. On Windows, Docker is always used regardless of this setting.

> **Tip:** The Quickstart skipped these prompts using `--define` flags:
> ```bash
> icp new my-project --subfolder hello-world \
>   --define backend_type=motoko \
>   --define frontend_type=react \
>   --define network_type=Default
> ```

Templates are fetched from the [icp-cli-templates](https://github.com/dfinity/icp-cli-templates) repository by default. You can also [create your own templates](guides/creating-templates.md).

Enter the project directory:

```bash
cd my-project
```

Your project contains:
- `icp.yaml` — Project configuration (canisters, networks, environments)
- `backend/` — Motoko source code
- `frontend/` — React application
- `README.md` — Project-specific instructions

## Start the Local Network

```bash
icp network start -d
```

This starts a local Internet Computer replica on your machine. The `-d` flag runs it in the background (detached) so you can continue using your terminal.

Verify the network is running:

```bash
icp network status
```

> **Note:** For local development, icp-cli uses an **anonymous identity** by default. This identity is automatically funded with ICP and cycles on local networks, so you can deploy immediately without setting up a wallet. For mainnet deployment, you'll create a dedicated identity — see [Deploying to Mainnet](guides/deploying-to-mainnet.md).

## Deploy

```bash
icp deploy
```

This single command:
1. **Builds** your Motoko code into WebAssembly (WASM)
2. **Builds** your React frontend
3. **Creates** canisters on the local network
4. **Installs** your code into the canisters

After deployment, you'll see output like:

```
Deployed canisters:
  backend (Candid UI): http://...localhost:8000/?id=...
  frontend: http://...localhost:8000
```

## Explore Your App

### Frontend

Open the **frontend URL** in your browser. You'll see a React app that calls your backend canister.

### Candid UI

Open the **Candid UI URL** (shown next to "backend"). Candid UI is a web interface that lets you interact with any canister that has a known [Candid](https://docs.internetcomputer.org/building-apps/interact-with-canisters/candid/candid-concepts) interface — no frontend code required.

Try it:
1. Find the `greet` method
2. Enter a name (e.g., "World")
3. Click "Call"
4. See the response: `"Hello, World!"`

Candid UI works with any backend canister, not just this example. It's useful for:
- Testing methods during development
- Exploring what methods a canister exposes
- Debugging without writing frontend code

### Command Line

You can also call your backend from the terminal:

```bash
icp canister call backend greet '("World")'
```

You should see: `("Hello, World!")`

The argument format `'("World")'` is [Candid](https://docs.internetcomputer.org/building-apps/interact-with-canisters/candid/candid-concepts) — the interface description language for the Internet Computer.

### Interactive Arguments

Don't want to type Candid manually? Omit the argument and icp-cli will prompt you interactively:

```bash
icp canister call backend greet
```

You'll see a prompt asking for the `name` parameter — just type `World` and press Enter. This works for any method with any argument types, making it easy to explore canister APIs without memorizing Candid syntax.

## Stop the Network

When you're done:

```bash
icp network stop
```

## Next Steps

You've deployed a full-stack app on the Internet Computer! Continue your journey:

- [Local Development](guides/local-development.md) — Learn the day-to-day development workflow
- [Deploying to Mainnet](guides/deploying-to-mainnet.md) — Go live on the Internet Computer
- [Project Model](concepts/project-model.md) — Understand how icp-cli organizes projects

[Browse all documentation →](index.md)
