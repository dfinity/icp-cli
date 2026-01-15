# icp-cli

A command-line tool for building and deploying applications on the Internet Computer.

## Quick Start

```bash
# Install via Homebrew
brew install dfinity/tap/icp-cli

# Or build from source
git clone https://github.com/dfinity/icp-cli.git
cd icp-cli && cargo build --release
export PATH=$(pwd)/target/release:$PATH

# Create and deploy a project
icp new my-project && cd my-project
icp network start -d
icp deploy
icp canister call my-canister greet '("World")'
```

## Documentation

- **[Tutorial](docs/tutorial.md)** — Deploy your first canister
- **[Guides](docs/guides/index.md)** — How to accomplish common tasks
- **[Concepts](docs/concepts/index.md)** — Understand how icp-cli works
- **[Reference](docs/reference/index.md)** — Complete CLI and configuration reference

## Examples

The [`examples/`](examples/) directory contains project templates:

- `icp-motoko/` — Motoko canister
- `icp-rust/` — Rust canister
- `icp-static-assets/` — Static website
- `icp-environments/` — Multi-environment setup

[View all examples →](examples/)

## Prerequisites

**Language-specific toolchains:**
- **For Rust canisters** — [Rust](https://rustup.rs/) and `rustup target add wasm32-unknown-unknown`
- **For Motoko canisters** — [mops](https://cli.mops.one/) and `mops toolchain init`

**Building from source** (not needed if installing via Homebrew):
- **Rust** — Install via [rustup](https://rustup.rs/) (Rust 2024 edition)

## Getting Help

- **[Documentation](docs/index.md)** — Guides, concepts, and reference
- **[GitHub Issues](https://github.com/dfinity/icp-cli/issues)** — Bug reports and feature requests
- **[Developer Forum](https://forum.dfinity.org/)** — Questions and discussions
- **[Discord](https://discord.internetcomputer.org)** — Real-time community chat

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

```bash
# Build
cargo build

# Test
cargo test

# Generate CLI docs
./scripts/generate-cli-docs.sh
```

## License

[Apache-2.0](LICENSE)
