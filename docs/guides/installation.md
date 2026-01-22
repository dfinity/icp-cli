# Installation

Install icp-cli on macOS, Linux, or Windows (WSL).

## macOS

```bash
brew install dfinity/tap/icp-cli
```

**Bash/Curl**

```bash
# install icp-cli
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/dfinity/icp-cli/releases/download/v0.1.0-beta.3/icp-cli-installer.sh | sh

# install ic-wasm which is a dependency for many recipes
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/dfinity/ic-wasm/releases/download/0.9.10/ic-wasm-installer.sh | sh
```

**PowerShell (Windows)**

```ps1
# install icp-cli
powershell -ExecutionPolicy Bypass -c "irm https://github.com/dfinity/icp-cli/releases/download/v0.30.3/cargo-dist-installer.ps1 | iex"

# install ic-wasm which is a dependency for many recipes
powershell -ExecutionPolicy Bypass -c "irm https://github.com/dfinitiy/ic-wasm/releases/download/v0.9.11/ic-wasm-installer.ps1 | iex"
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

To update later:

```bash
brew upgrade dfinity/tap/icp-cli
```

## Linux / WSL

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/dfinity/icp-cli/releases/download/v0.1.0-beta.3/icp-cli-installer.sh | sh
```

The installer adds icp-cli to your PATH automatically. Restart your shell or run the `source` command shown by the installer.

## Windows

```ps1
powershell -ExecutionPolicy Bypass -c "irm https://github.com/dfinity/icp-cli/releases/download/v0.1.0-beta.3/icp-cli-installer.ps1 | iex"
```

The installer adds icp-cli to your PATH automatically. Restart your shell (and if it's inside another program, e.g. the VS Code embedded shell, restart that program too).

## Verify Installation

```bash
icp --version
```

## Language Toolchains

icp-cli uses your language's compiler to build canisters. Install what you need:

**Rust canisters:**

```bash
rustup target add wasm32-unknown-unknown
```

**Motoko canisters:**

```bash
npm install -g ic-mops
mops toolchain init
```

## Other dependencies

### Docker/WSL2 (Windows)

On Windows, the local network will be run in a Docker container inside WSL2. It is recommended to install [Docker Desktop](https://www.docker.com/products/docker-desktop/) with WSL2 integration, but a manually run `dockerd` instance is [also supported](docs/containers.md).

Docker is also a dependency for projects that manually configure their network to be container-based.

## Troubleshooting

**"command not found: icp" (after curl install)**

The binary isn't in your PATH. Add this to your shell config (`~/.bashrc`, `~/.zshrc`, etc.):

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Then restart your shell or run `source ~/.bashrc` (or `~/.zshrc`).

**Network launcher download fails**

The network launcher downloads automatically on first use. If it fails:
- Check your internet connection
- Try again (transient failures are possible)
- Download manually from [icp-cli-network-launcher releases](https://github.com/dfinity/icp-cli-network-launcher/releases) and set `ICP_CLI_NETWORK_LAUNCHER_PATH`

## Next Steps

- [Tutorial](../tutorial.md) — Deploy your first canister
- [Local Development](local-development.md) — Day-to-day workflow

[Browse all documentation →](../index.md)
