# Frontend Environment Variables Example

This example demonstrates how to pass environment variables from the asset canister to the frontend webapp.

## Overview

This project consists of two canisters:

- [backend](./backend/): a pre-built Hello World canister, together with its [`backend.did`](./backend/dist/hello_world.did) file.
- [frontend](./frontend/): a [Vite](https://vite.dev/) webapp deployed in an asset canister.

### Bindings Generation

The [`@icp-sdk/bindgen`](https://npmjs.com/package/@icp-sdk/bindgen) library offers a plugin for Vite that generates the TypeScript Candid bindings from the [`backend.did`](./backend/dist/hello_world.did) file. The bindings are generated at build time by Vite and are saved in the [`frontend/app/src/backend/api/`](./frontend/app/src/backend/api/) folder.

The plugin supports hot module replacement, so you can run the frontend in development mode (by running `npm run dev` in the [`frontend/app/`](./frontend/app/) folder) and make changes to the Candid declaration file to see the bindings being updated in real time.

See the [`vite.config.ts`](./frontend/app/vite.config.ts) file for how the plugin is used.

### Environment Variables

The project is configured to pass the backend's canister ID to the frontend using a [cookie](https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/Cookies). The environment variables are made available to the frontend JS code that runs in the browser by following this flow:

1. During deployment, the `icp` CLI sets the backend's canister ID in the frontend canister's (asset canister) environment variables
2. The asset canister sets a specific cookie (named `ic_env`) that contains some environment variables when the assets are uploaded
3. The asset canister serves the frontend assets with the cookie set
4. The frontend JS code uses the [`@icp-sdk/core/agent/canister-env`](https://js.icp.build/core/latest/canister-env/) module to parse the cookie and extract the environment variables

In the [`App.tsx`](./frontend/app/src/App.tsx) file, you can see how the frontend can obtain the backend's canister ID from the environment variables and use it to create an actor for the backend canister.

## Prerequisites

Before you begin, ensure that you have the following installed:

- [Node.js](https://nodejs.org/)
- [npm](https://docs.npmjs.com/)

## Instructions

First, start a local network in a separate terminal window:

```bash
icp network run
```

Then, deploy both canisters:

```bash
icp deploy
```

This command will output something like this:

```bash
Syncing canisters:
[backend] ✔ Synced successfully: uqqxf-5h777-77774-qaaaa-cai
[frontend] ✔ Synced successfully: uxrrr-q7777-77774-qaaaq-cai # <- copy this canister id
```

Finally, open the deployed frontend in a browser. Copy the frontend's canister ID from the output of the `icp deploy` command and construct the URL in this way: `http://<frontend_canister_id>.localhost:8000/`.
