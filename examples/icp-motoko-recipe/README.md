# ICP Motoko Recipe Example

This example demonstrates how to use the built-in `motoko` recipe type to build Motoko canisters.

## Overview

The `motoko` recipe type provides a streamlined way to build Motoko canisters using the Motoko compiler (`moc`). This is one of the built-in recipe types provided by ICP-CLI for common canister development workflows.

## Configuration

The [`icp.yaml`](./icp.yaml) file configures a canister using the `@dfinity/motoko` recipe:

```yaml
canisters:
  - name: my-canister
    recipe:
      type: "@dfinity/motoko"
      configuration:
        main: src/main.mo
        args: --incremental-gc
```

### Key Components

- **`type: "@dfinity/motoko"`**: Uses the official DFINITY Motoko recipe
- **`main`**: Specifies the main Motoko source file (required)
- **`args`**: Compiler flags passed to `moc` (optional)

## Source Code

The [`src/main.mo`](./src/main.mo) file contains the Motoko canister implementation. This is the entry point that gets compiled by the `moc` compiler.

## How It Works

1. ICP-CLI uses the built-in `motoko` recipe resolver
2. The resolver generates build steps that:
   - Check for `moc` compiler availability
   - Compile the Motoko source code using `moc`
   - Move the resulting WASM to the output path
3. The canister is built and ready for deployment

## Prerequisites

- Motoko compiler (`moc`) must be installed
- Install via: <https://internetcomputer.org/docs/building-apps/getting-started/install>

## Use Cases

- Standard Motoko canister development
- Simple build workflows for Motoko projects
- Projects that don't require custom build logic

## Related Examples

- [`icp-motoko`](../icp-motoko/): Motoko example with explicit build/sync configuration
- [`icp-motoko-mops`](../icp-motoko-mops/): Motoko with MOPS package manager
- [`icp-rust-recipe`](../icp-rust-recipe/): Rust equivalent using recipe type
