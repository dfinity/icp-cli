# Wasm Optimization Recipe Example

This example demonstrates how to use canister recipes to deploy a pre-built Wasm module with optimization settings.

## Overview

This project showcases the recipe-based approach for optimizing Wasm modules during deployment. The recipe uses the `prebuilt` type with optimization configuration to reduce canister size and deployment cost.

The configuration in `icp.yaml` shows:

- **Recipe Type**: `prebuilt` - Uses an existing Wasm file
- **Path**: Reference to a pre-built Wasm module from another example
- **SHA256**: Hash verification for integrity checking
- **Shrink**: Enables Wasm module size reduction
- **Compress**: Enables gzip compression of the Wasm module

## Key Features

- Declarative optimization configuration using recipes
- Automatic Wasm shrinking to reduce module size
- Gzip compression for further size reduction
- No need for external tools or custom build scripts

## Optimization Benefits

The optimization settings provide:

- **Shrink**: Removes unnecessary sections and optimizes the Wasm binary
- **Compress**: Applies gzip compression to reduce deployment size and cost

These optimizations can significantly reduce canister deployment size and associated costs on the Internet Computer.

## Instructions

First, start a local network in a separate terminal window:

```bash
icp network start
```

Then, deploy the canister:

```bash
icp deploy
```

The canister will be deployed with the optimized and compressed Wasm module.

## Recipe vs Build Adapters

This example uses the newer recipe system (`recipe:`) instead of build adapters (`build:`). The recipe approach simplifies Wasm optimization compared to manual tool invocation, making it more declarative and easier to configure.
