# Static Assets Example

This example demonstrates how to deploy a canister that serves static assets using `icp`.

## Overview

This project uses a pre-built asset storage canister to serve the contents of the `www` directory. The `icp.yaml` file is configured to sync the `www` directory with the canister.

## Instructions

First, start a local network in a separate terminal window:

```bash
icp network run
```

Then, deploy the canister and sync the assets:

```bash
icp deploy
```

Once the canister is deployed, you can access the `index.html` file in your browser.

The `icp network run` command will output the port number for the local network (e.g., `8000`), and the `icp deploy` command will output the canister ID. You can then construct the URL to view the site, which will look something like this:

`http://localhost:8000/?canisterId=<canister_id>`
