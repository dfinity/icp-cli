# Static React Site Example

This example demonstrates how to build and deploy a static React site using `icp`.

## Overview

This project is a simple React application that is built using Vite. The `icp.yaml` file is configured to build the application and then sync the resulting `dist` directory to an asset storage canister.

## Prerequisites

Before you begin, ensure that you have Node.js and npm installed.

## Instructions

First, install the project dependencies:

```bash
npm ci
```

Next, start a local network in a separate terminal window:

```bash
icp network start
```

Then, deploy the canister:

```bash
icp deploy
```

Once the canister is deployed, you can access the React application in your browser.

The `icp network start` command will output the port number for the local network (e.g., `8000`), and the `icp deploy` command will output the canister ID. You can then construct the URL to view the site, which will look something like this:

`http://localhost:8000/?canisterId=<canister_id>`
