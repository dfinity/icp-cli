# Installation

Install icp-cli on macOS, Linux, or Windows (WSL).

## macOS

```bash
brew install dfinity/tap/icp-cli
```

To update later:

```bash
brew upgrade dfinity/tap/icp-cli
```

## Linux / Windows (WSL)

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/dfinity/icp-cli/releases/latest/download/icp-cli-installer.sh | sh
```

The installer adds icp-cli to your PATH automatically. Restart your shell or run the source command shown by the installer.

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
