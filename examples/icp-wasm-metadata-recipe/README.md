# Wasm Metadata Recipe Example

This example demonstrates how to use canister recipes to deploy a pre-built Wasm module with custom metadata.

## Overview

This project showcases the recipe-based approach for adding custom metadata to Wasm modules during deployment. The recipe uses the `prebuilt` type with additional metadata configuration.

The configuration in `icp.yaml` shows:

- **Recipe Type**: `prebuilt` - Uses an existing Wasm file
- **Path**: Reference to a pre-built Wasm module from another example
- **SHA256**: Hash verification for integrity checking
- **Metadata**: Custom key-value pairs embedded in the Wasm module

## Key Features

- Declarative metadata configuration using recipes
- Support for custom user-defined metadata fields
- Wasm integrity verification with SHA256 checksums
- No need for external tools like `ic-wasm`

## Metadata Configuration

The example adds two metadata entries:

- `user:name`: "Hank Azaria"
- `user:age`: "61"

These demonstrate how arbitrary metadata can be embedded in the Wasm module for later retrieval.

## Instructions

First, start a local network in a separate terminal window:

```bash
icp network run
```

Then, deploy the canister:

```bash
icp deploy
```

The canister will be deployed with the specified metadata embedded in the Wasm module.

## Recipe vs Build Adapters

This example uses the newer recipe system (`recipe:`) instead of build adapters (`build:`). The recipe approach simplifies metadata addition compared to manual `ic-wasm` commands, making it more declarative and easier to maintain.
