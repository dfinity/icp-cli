# ICP Rust Recipe Example

This example demonstrates how to use the built-in `rust` recipe type to build Rust canisters.

## Overview

The `rust` recipe type provides a streamlined way to build Rust canisters using Cargo with the WASM target. This is one of the built-in recipe types provided by ICP-CLI for common canister development workflows.

## Configuration

The [`icp.yaml`](./icp.yaml) file configures a canister using the built-in `rust` recipe:

```yaml
canister:
  name: my-canister
  recipe:
    type: rust
    configuration:
      package: icp-rust-recipe
```

### Key Components

- **`type: rust`**: Uses the built-in Rust recipe type
- **`package`**: Specifies the Cargo package name to build (required)

## Project Structure

- [`Cargo.toml`](./Cargo.toml): Cargo project configuration with WASM target setup
- [`src/lib.rs`](./src/lib.rs): Rust canister implementation using ic-cdk
- [`.gitignore`](./gitignore): Git ignore rules for Rust projects

## How It Works

1. ICP-CLI uses the built-in `rust` recipe resolver
2. The resolver generates build steps that:
   - Build the specified Cargo package with `--target wasm32-unknown-unknown --release`
   - Move the resulting WASM file to the output path
   - Inject metadata about the Cargo version used
3. The canister is built and ready for deployment

## Prerequisites

- Rust toolchain with WASM target support
- Install via: <https://rustup.rs/>
- Add WASM target: `rustup target add wasm32-unknown-unknown`

## Build Process

The recipe automatically:

1. Compiles the Rust code to WASM using Cargo
2. Extracts the WASM binary from the target directory
3. Renames it to match ICP-CLI's expected output format
4. Injects build metadata using ic-wasm

## Use Cases

- Standard Rust canister development
- Projects using ic-cdk for Internet Computer development
- Canisters that don't require custom build logic
- Multi-package Rust workspaces

## Related Examples

- [`icp-rust`](../icp-rust/): Rust example with explicit build/sync configuration
- [`icp-motoko-recipe`](../icp-motoko-recipe/): Motoko equivalent using recipe type
