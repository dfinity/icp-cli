# Tutorial

Deploy your first canister on the Internet Computer in under 10 minutes.

## Prerequisites

**Language-specific toolchains:**
- For Rust canisters: [Rust](https://rustup.rs/) and `rustup target add wasm32-unknown-unknown`
- For Motoko canisters: [mops](https://cli.mops.one/) (run `mops toolchain init` after installation)

**Building icp-cli from source** (not needed if installing via Homebrew):
- [Rust](https://rustup.rs/) — The Rust toolchain

## Install icp-cli

**Via Homebrew (recommended):**

```bash
brew install dfinity/tap/icp-cli
```

**From source:**

```bash
git clone https://github.com/dfinity/icp-cli.git
cd icp-cli && cargo build --release
export PATH=$(pwd)/target/release:$PATH
```

Verify installation:

```bash
icp help
```

## Create a Project

```bash
icp new my-project
```

Select a template when prompted, then enter the project directory:

```bash
cd my-project
```

Your project contains:
- `icp.yaml` — Project configuration
- `src/` — Source code
- `README.md` — Project-specific instructions

## Start the Local Network

```bash
icp network start -d
```

This starts a local Internet Computer network in the background.

## Deploy

```bash
icp deploy
```

This single command:
1. **Builds** your source code into WebAssembly (WASM)
2. **Creates** a canister on the local network
3. **Installs** your WASM code

**Tip:** You can also run `icp build` separately if you want to verify compilation before deploying.

## Interact with Your Canister

```bash
icp canister call my-canister greet '("World")'
```

You should see: `("Hello, World!")`

## Stop the Network

When you're done:

```bash
icp network stop
```

## Next Steps

You've deployed your first canister. Now:

- [Core Concepts](concepts/project-model.md) — Understand how icp-cli works
- [Local Development](guides/local-development.md) — Learn the day-to-day workflow

[Browse all documentation →](index.md)
