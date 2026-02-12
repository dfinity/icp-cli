# Frontend Environment Variables Example

This example demonstrates how frontends discover backend canisters and verify responses using the `ic_env` cookie mechanism.

## Overview

This project consists of two canisters:

- [backend](./backend/): a pre-built Hello World canister with its [Candid interface](./backend/dist/hello_world.did)
- [frontend](./frontend/): a [Vite](https://vite.dev/) React app deployed to an asset canister

## How It Works

When a frontend calls a backend canister, it needs two things:

1. **The backend's canister ID** — to know which canister to call
2. **The network's root key** — to verify response signatures

Both are provided via a cookie named `ic_env`. The [`@icp-sdk/core/agent/canister-env`](https://www.npmjs.com/package/@icp-sdk/core) module parses this cookie and returns:

- `PUBLIC_CANISTER_ID:backend` — the backend canister ID (string)
- `IC_ROOT_KEY` — the network's root key (Uint8Array, converted from hex internally)

See [`App.tsx`](./frontend/app/src/App.tsx) for the implementation.

### Who Sets the Cookie?

**Asset canister (production):** When you deploy with `icp deploy`, the CLI sets `PUBLIC_CANISTER_ID:*` variables on all canisters. The asset canister then serves the `ic_env` cookie containing these variables plus the network's root key.

**Dev server (development):** The Vite dev server mimics this behavior by setting the same cookie. See [`vite.config.ts`](./frontend/app/vite.config.ts) for how it fetches the canister ID, root key, and API URL from the CLI.

### Bindings Generation

The [`@icp-sdk/bindgen`](https://npmjs.com/package/@icp-sdk/bindgen) Vite plugin generates TypeScript bindings from the Candid interface at build time. Output is saved to [`frontend/app/src/backend/api/`](./frontend/app/src/backend/api/).

## Prerequisites

- [Node.js](https://nodejs.org/) and npm
- [icp-cli](https://github.com/dfinity/icp-cli)

## Running the Example

### Option 1: Deploy to Asset Canister

Start a local network:

```bash
icp network start
```

Deploy both canisters:

```bash
icp deploy
```

Open the frontend at `http://<frontend-canister-id>.localhost:8000/` (the canister ID is shown in the deploy output).

### Option 2: Use the Dev Server (Hot Reloading)

Start a local network and deploy only the backend:

```bash
icp network start
icp deploy backend
```

> **Important:** The backend must be deployed before starting the dev server. The dev server needs to fetch the backend's canister ID to configure the `ic_env` cookie. If the backend isn't deployed, you'll see an error with instructions.

Install dependencies and start the dev server:

```bash
cd frontend/app
npm install
npm run dev
```

Open `http://localhost:5173/`. The dev server automatically configures the `ic_env` cookie and proxies API requests to the local network.

### Targeting Different Environments

The dev server supports any environment defined in your project:

```bash
# Local (default)
npm run dev

# Custom environment
ICP_ENVIRONMENT=staging npm run dev

# IC mainnet
ICP_ENVIRONMENT=ic npm run dev
```

The [`vite.config.ts`](./frontend/app/vite.config.ts) uses `icp network status` to fetch the correct root key and API URL for the target environment.

## Learn More

- [Canister Discovery](../../docs/concepts/canister-discovery.md) — How the `ic_env` cookie mechanism works
- [Local Development Guide](../../docs/guides/local-development.md#frontend-development) — Frontend development workflow
