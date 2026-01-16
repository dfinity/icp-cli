# Tutorial

Deploy your first canister on the Internet Computer in under 10 minutes.

## Prerequisites

Install icp-cli and the toolchain for your canister language.

### Install icp-cli

**Via Homebrew (macOS):**

```bash
brew install dfinity/tap/icp-cli
```

**From source:**

```bash
git clone https://github.com/dfinity/icp-cli.git
cd icp-cli && cargo build --release
export PATH=$(pwd)/target/release:$PATH
```

For detailed installation options, see the [Installation Guide](guides/installation.md).

### Language Toolchains

Install the toolchain for the language you'll use:

- **Rust canisters**: [Rust](https://rustup.rs/) and `rustup target add wasm32-unknown-unknown`
- **Motoko canisters**: [mops](https://cli.mops.one/) and `mops toolchain init`

### Verify Installation

```bash
icp --version
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

First, find your canister name:

```bash
icp canister list
```

Then call a method on it (replace `<canister-name>` with your actual canister name):

```bash
icp canister call <canister-name> greet '("World")'
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
