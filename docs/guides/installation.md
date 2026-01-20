# Installation

This guide covers all methods for installing icp-cli on your system.

## Quick Install

**macOS (Homebrew):**

```bash
brew install dfinity/tap/icp-cli
```

**Curl**

```bash
# install icp-cli
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/dfinity/icp-cli/releases/download/v0.1.0-beta.3/icp-cli-installer.sh | sh

# install ic-wasm which is a dependency for many recipes
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/dfinity/ic-wasm/releases/download/0.9.10/ic-wasm-installer.sh | sh
```

**From source:**

Cargo is required as a pre-requisite.

```bash
git clone https://github.com/dfinity/icp-cli.git
cd icp-cli && cargo build --release
export PATH=$(pwd)/target/release:$PATH
```

Verify installation:

```bash
icp --version
```

## Installation Methods

### Homebrew (macOS)

The recommended installation method for macOS:

```bash
brew install dfinity/tap/icp-cli
```

To update:

```bash
brew upgrade dfinity/tap/icp-cli
```

### Downloading binaries

You can download binaries for your platform:

- icp-cli at https://github.com/dfinity/icp-cli/releases
- ic-wasm at https://github.com/dfinity/ic-wasm/releases

Alternatively, you can curl and run the installation scripts:

```bash
# install icp-cli
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/dfinity/icp-cli/releases/download/v0.1.0-beta.3/icp-cli-installer.sh | sh

# install ic-wasm which is a dependency for many recipes
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/dfinity/ic-wasm/releases/download/0.9.10/ic-wasm-installer.sh | sh
```

### Building from Source

Building from source works on macOS, Linux, and Windows (WSL).

#### Prerequisites

**Rust toolchain:**

Install Rust via [rustup](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

icp-cli requires Rust 1.88.0 or later (Rust 2024 edition).

**Platform-specific dependencies:**

| Platform | Dependencies |
|----------|--------------|
| macOS | Xcode Command Line Tools: `xcode-select --install` |
| Ubuntu/Debian | `sudo apt install build-essential pkg-config libssl-dev` |
| Fedora/RHEL | `sudo dnf install gcc pkg-config openssl-devel` |
| Arch Linux | `sudo pacman -S base-devel openssl` |

#### Build Steps

Clone and build:

```bash
git clone https://github.com/dfinity/icp-cli.git
cd icp-cli
cargo build --release
```

The binary is at `target/release/icp`. Add it to your PATH:

```bash
# Add to current session
export PATH=$(pwd)/target/release:$PATH

# Or copy to a location in your PATH
cp target/release/icp ~/.local/bin/
```

To update, pull the latest changes and rebuild:

```bash
git pull
cargo build --release
```

### Cargo Install

If icp-cli is published to crates.io:

```bash
cargo install icp-cli
```

## Language Toolchains

icp-cli builds canisters using your language's toolchain. Install the toolchains for the languages you'll use:

### Rust Canisters

Install the WebAssembly target:

```bash
rustup target add wasm32-unknown-unknown
```

### Motoko Canisters

Install [mops](https://cli.mops.one/) and initialize the toolchain:

```bash
# Install mops (see https://cli.mops.one/ for latest instructions)
npm install -g ic-mops

# Initialize Motoko toolchain
mops toolchain init
```

## Verifying Installation

After installation, verify everything works:

```bash
# Check icp-cli version
icp --version

# View available commands
icp help

# Test creating a project (optional)
icp new test-project
cd test-project
icp network start -d
icp deploy
icp network stop
cd ..
rm -rf test-project
```

## Troubleshooting

**"command not found: icp"**

The binary isn't in your PATH. Either:
- Add the directory containing `icp` to your PATH
- Use the full path to the binary

**Build fails with OpenSSL errors**

Install OpenSSL development libraries for your platform (see prerequisites above).

**Build fails with "rustc too old"**

Update Rust:

```bash
rustup update
```

**Network launcher download fails**

The network launcher is automatically downloaded on first use. If it fails:
- Check your internet connection
- Try again — transient failures are possible
- For manual installation, download from [github.com/dfinity/icp-cli-network-launcher/releases](https://github.com/dfinity/icp-cli-network-launcher/releases) and set `ICP_CLI_NETWORK_LAUNCHER_PATH`

## Next Steps

- [Tutorial](../tutorial.md) — Deploy your first canister
- [Local Development](local-development.md) — Day-to-day workflow

[Browse all documentation →](../index.md)
