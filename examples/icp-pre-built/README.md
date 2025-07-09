# Pre-built Canister Example

This example demonstrates how to deploy a pre-built canister using `icp`.

## Overview

This project deploys a canister that has been previously built and is located in the `dist` directory. The `icp.yaml` file specifies the path to the Wasm module and its SHA256 hash to ensure its integrity.

## Instructions

First, start a local network in a separate terminal window:

```bash
icp network run
```

Then, deploy the canister:

```bash
icp deploy
```
