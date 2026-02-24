# Installation

Set up everything you need to build and deploy canisters on the Internet Computer.

**What you'll install:**

| Tool                   | Purpose                                         |
|------------------------|-------------------------------------------------|
| **icp-cli**            | Core CLI for building and deploying canisters   |
| **ic-wasm**            | Optimizes WebAssembly for the Internet Computer |
| **Language toolchain** | Motoko compiler (via mops) or Rust compiler     |

> **Windows users:** Local networks require [Docker Desktop](https://docs.docker.com/desktop/setup/install/windows-install/), and Motoko requires [WSL](https://learn.microsoft.com/en-us/windows/wsl/install). For the full experience, install both and run commands inside WSL. Rust-only projects deploying to mainnet can run natively on Windows.

> **Linux users:** The pre-compiled binary requires system libraries that may be missing on minimal installs. If installation fails or `icp` won't start, install these dependencies:
> ```bash
> # Ubuntu/Debian
> sudo apt-get install -y libdbus-1-3 libssl3 ca-certificates
> # Fedora/RHEL
> sudo dnf install -y dbus-libs openssl ca-certificates
> ```

## Quick Install via npm (Recommended)

**Required:** [Node.js](https://nodejs.org/) (LTS) — needed for npm and for building frontend canisters.

**1. Install the core tools:**

```bash
npm install -g @icp-sdk/icp-cli @icp-sdk/ic-wasm
```

**2. Install your language toolchain:**

**Motoko:**

```bash
npm install -g ic-mops
```

**Rust** (if not already installed):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-unknown-unknown
```

**3. Verify installation:**

```bash
icp --version
ic-wasm --version
```

---

## Alternative Installation Methods

If you prefer not to use npm, or need platform-specific options, see the sections below.

### icp-cli

**Homebrew (macOS/Linux):**

```bash
brew install icp-cli
```

**Shell Script (macOS/Linux/WSL):**

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/dfinity/icp-cli/releases/latest/download/icp-cli-installer.sh | sh
```

**Shell Script (Windows):**

```ps1
powershell -ExecutionPolicy Bypass -c "irm https://github.com/dfinity/icp-cli/releases/latest/download/icp-cli-installer.ps1 | iex"
```

### ic-wasm

`ic-wasm` is a WebAssembly post-processing tool that optimizes canisters for the Internet Computer. It provides:
- **Optimization**: ~10% cycle reduction for Motoko, ~4% for Rust
- **Size reduction**: ~16% smaller binaries for both languages
- **Metadata**: Embed Candid interfaces and version information
- **Shrinking**: Remove unused code and debug symbols

**When is it needed?**
- **Required** if using official templates (motoko, rust, hello-world) — all backend templates use recipes that depend on ic-wasm
- **Required** if using official recipes (`@dfinity/motoko`, `@dfinity/rust`) — these recipes inject required metadata using ic-wasm
- **Not required** if building canisters with custom script steps that don't invoke ic-wasm

**Installation:**

**Homebrew (macOS/Linux):**

```bash
brew install ic-wasm
```

**Shell Script (macOS/Linux):**

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/dfinity/ic-wasm/releases/latest/download/ic-wasm-installer.sh | sh
```

**Shell Script (Windows):**

```ps1
powershell -ExecutionPolicy Bypass -c "irm https://github.com/dfinity/ic-wasm/releases/latest/download/ic-wasm-installer.ps1 | iex"
```

Learn more: [ic-wasm repository](https://github.com/dfinity/ic-wasm)

### Language Toolchains

**Motoko:**

```bash
curl -fsSL cli.mops.one/install.sh | sh
```

> **Note:** Requires [Node.js](https://nodejs.org/) and a package manager (npm, pnpm, or bun). The shell script installs the latest Mops version stored onchain on ICP.

**Rust:**

Install from [rustup.rs](https://rustup.rs/):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add wasm32-unknown-unknown
```


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
