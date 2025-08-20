# Pre-Built Recipe Example

This example demonstrates how to use canister recipes to deploy a pre-built Wasm module.

## Overview

This project showcases the recipe-based approach for deploying canisters using the `prebuilt` recipe type. The recipe system provides a declarative way to configure canister deployment without requiring custom build steps.

The configuration in `icp.yaml` shows:

- **Recipe Type**: `prebuilt` - Uses an existing Wasm file
- **Path**: Reference to a pre-built Wasm module from another example
- **SHA256**: Hash verification for integrity checking

## Key Features

- Simple declarative configuration using recipes
- Wasm integrity verification with SHA256 checksums
- References external pre-built modules for reuse

## Instructions

First, start a local network in a separate terminal window:

```bash
icp network run
```

Then, deploy the canister:

```bash
icp deploy
```

The canister will be deployed using the pre-built Wasm module specified in the recipe configuration.

## Recipe vs Build Adapters

This example uses the newer recipe system (`recipe:`) instead of build adapters (`build:`). Recipes provide a more declarative approach to canister configuration and are the recommended method for new projects.
