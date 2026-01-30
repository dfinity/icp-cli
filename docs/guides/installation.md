# Installation

Set up everything you need to build and deploy canisters on the Internet Computer.

This guide covers:
- Prerequisites (Node.js)
- Installing icp-cli (the core tool)
- Installing language toolchains (Rust or Motoko compilers)
- Installing ic-wasm for optimization

## Prerequisites

[Node.js](https://nodejs.org/) (LTS recommended) is required for:
- Installing the Motoko toolchain
- Building frontend canisters

> **Rust-only projects:** If you're only building Rust backend canisters without a frontend, you can skip Node.js.

## Install icp-cli

**macOS / Linux / WSL:**

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/dfinity/icp-cli/releases/download/v0.1.0-beta.6/icp-cli-installer.sh | sh
```

Restart your shell or follow the instructions shown by the installer.

**Windows:**

```ps1
powershell -ExecutionPolicy Bypass -c "irm https://github.com/dfinity/icp-cli/releases/download/v0.1.0-beta.6/icp-cli-installer.ps1 | iex"
```

Restart your terminal after installation.

> **Windows notes:**
> - **Local networks** require [Docker Desktop](https://docs.docker.com/desktop/setup/install/windows-install/)
> - **Motoko canisters** require [WSL](https://learn.microsoft.com/en-us/windows/wsl/install) — the Motoko compiler doesn't run natively on Windows. Install icp-cli inside WSL and follow the macOS/Linux instructions instead.
> - **Rust canisters** work natively on Windows without WSL

**Alternative: Homebrew (macOS only)**

```bash
brew install dfinity/tap/icp-cli
```

To update later: `brew upgrade dfinity/tap/icp-cli`

### Verify Installation

```bash
icp --version
```

## Install Language Toolchains

icp-cli uses your language's compiler to build canisters. Install what you need:

**Rust canisters:**

If you don't have Rust installed, install it from [rustup.rs](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Then add the WebAssembly target:

```bash
rustup target add wasm32-unknown-unknown
```

**Motoko canisters:**

```bash
npm install -g ic-mops
mops toolchain init
```

## Install ic-wasm (Required for templates and recipes)

`ic-wasm` is a WebAssembly post-processing tool that optimizes canisters for the Internet Computer. It provides:
- **Optimization**: ~10% cycle reduction for Motoko, ~4% for Rust
- **Size reduction**: ~16% smaller binaries for both languages
- **Metadata**: Embed Candid interfaces and version information
- **Shrinking**: Remove unused code and debug symbols

**When is it needed?**
- **Required** if using official templates (motoko, rust, hello-world) - all backend templates use recipes that depend on ic-wasm
- **Required** if using official recipes (`@dfinity/motoko`, `@dfinity/rust`) - these recipes inject required metadata using ic-wasm
- **Not required** if building canisters with custom script steps that don't invoke ic-wasm

**Installation:**

**Note:** If you installed icp-cli via Homebrew, ic-wasm is already installed as a dependency. Skip this section.

**macOS/Linux:**
```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/dfinity/ic-wasm/releases/latest/download/ic-wasm-installer.sh | sh
```

**Windows:**
```ps1
powershell -ExecutionPolicy Bypass -c "irm https://github.com/dfinity/ic-wasm/releases/latest/download/ic-wasm-installer.ps1 | iex"
```

Learn more: [ic-wasm repository](https://github.com/dfinity/ic-wasm)


## Troubleshooting

**"command not found: icp" (after curl install)**

The binary isn't in your PATH. Add this to your shell config (`~/.bashrc`, `~/.zshrc`, etc.):

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Then restart your shell or run `source ~/.bashrc` (or `~/.zshrc`).

**"Cannot connect to Docker" (Windows)**

On Windows, Docker Desktop must be running before starting a local network. Ensure:
- Docker Desktop is installed and running
- For manual `dockerd` setup with WSL2, see the [containerized networks guide](containerized-networks.md)

**Network launcher download fails**

The network launcher downloads automatically on first use. If it fails:
- Check your internet connection
- Try again (transient failures are possible)
- Download manually from [icp-cli-network-launcher releases](https://github.com/dfinity/icp-cli-network-launcher/releases) and set `ICP_CLI_NETWORK_LAUNCHER_PATH`

## Next Steps

- [Quickstart](../quickstart.md) — Deploy a full-stack app in under 5 minutes
- [Tutorial](../tutorial.md) — Understand each step in detail
- [Local Development](local-development.md) — Day-to-day workflow

[Browse all documentation →](../index.md)
