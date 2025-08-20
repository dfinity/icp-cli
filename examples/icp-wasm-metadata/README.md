# Wasm Metadata Example

This example demonstrates how to use build adapters to add custom metadata to a Wasm module before deploying it.

## Overview

This project showcases a multi-step build process defined in `icp.yaml`. The process involves:

1. Fetching a pre-built Wasm module using the `pre-built` adapter
2. Using `ic-wasm` to add custom metadata to the Wasm module

This approach allows for adding arbitrary metadata to Wasm modules during the build process.

## Prerequisites

Before you begin, ensure that you have the following tools installed:

- **ic-wasm**: A tool for post-processing Wasm modules for the Internet Computer.
  - Installation instructions: <https://github.com/dfinity/ic-wasm>

## Build Process

The build configuration uses two adapters:

1. **Pre-built adapter**: Fetches an existing Wasm file with SHA256 verification
2. **Script adapter**: Runs `ic-wasm` to add metadata fields to the module

The example adds a `user:name` metadata field with the value "Hank Azaria".

## Instructions

First, start a local network in a separate terminal window:

```bash
icp network run
```

Then, deploy the canister:

```bash
icp deploy
```

During the deployment, you will see output from the build adapters as they process the Wasm module and add the metadata.

## Build Adapters vs Recipes

This example uses the traditional build adapter system (`build:`). For a more declarative approach to adding metadata, see the `icp-wasm-metadata-recipe` example which uses the newer recipe system.
