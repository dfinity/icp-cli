# Wasm Optimization Example

This example demonstrates how to use build adapters to optimize a Wasm module before deploying it.

## Overview

This project showcases a multi-step build process defined in `icp.yaml`. The process involves:

1.  Fetching a pre-built Wasm module.
2.  Using `ic-wasm` to shrink the Wasm module, reducing its size.
3.  Compressing the optimized Wasm module with `gzip`.

This approach allows for significant reductions in canister deployment size and cost.

## Prerequisites

Before you begin, ensure that you have the following tools installed:

- **ic-wasm**: A tool for post-processing Wasm modules for the Internet Computer.

  - Installation instructions: https://github.com/dfinity/ic-wasm

- **gzip**: A standard file compression utility.
  - On macOS (with Homebrew): `brew install gzip`
  - On Debian/Ubuntu: `sudo apt-get install gzip`

## Instructions

First, start a local network in a separate terminal window:

```bash
icp network start
```

Then, deploy the canister:

```bash
icp deploy
```

During the deployment, you will see output from the build adapters as they optimize the Wasm module.
