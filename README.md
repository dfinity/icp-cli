# icp-cli

A command-line tool for building and deploying applications on the Internet Computer.

## Quick Start

```bash
# Install via Homebrew (macOS)
brew install dfinity/tap/icp-cli

# Create and deploy a project
icp new my-project && cd my-project
icp network start -d
icp deploy

# List your canisters and call a method (canister name depends on your template)
icp canister list
icp canister call <canister-name> greet '("World")'
```

See the [Installation Guide](docs/guides/installation.md) for all installation methods including building from source.

## For dfx Users

If you're coming from dfx (the previous Internet Computer SDK), see the **[Migration Guide](docs/migration/from-dfx.md)** for command mappings, workflow differences, and how to migrate existing projects.

## Documentation

- **[Tutorial](docs/tutorial.md)** — Deploy your first canister
- **[Guides](docs/guides/index.md)** — How to accomplish common tasks
- **[Concepts](docs/concepts/index.md)** — Understand how icp-cli works
- **[Reference](docs/reference/index.md)** — Complete CLI and configuration reference

## Examples

The [`examples/`](examples/) directory contains example projects to help you get started:

- `icp-motoko/` — Motoko canister
- `icp-rust/` — Rust canister
- `icp-static-assets/` — Static website
- `icp-environments/` — Multi-environment setup

[View all examples →](examples/)

## Prerequisites

**Language-specific toolchains** (install for the languages you'll use):
- **Rust canisters** — [Rust](https://rustup.rs/) and `rustup target add wasm32-unknown-unknown`
- **Motoko canisters** — [mops](https://cli.mops.one/) and `mops toolchain init`

## Getting Help

- **[Documentation](docs/index.md)** — Guides, concepts, and reference
- **[GitHub Issues](https://github.com/dfinity/icp-cli/issues)** — Bug reports and feature requests
- **[Developer Forum](https://forum.dfinity.org/)** — Questions and discussions
- **[Discord](https://discord.internetcomputer.org)** — Real-time community chat in #dx-feedback

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](.github/CONTRIBUTING.md) for guidelines.

```bash
# Build
cargo build

# Test
cargo test

# Generate CLI docs
./scripts/generate-cli-docs.sh

# Update the yaml file schemas
./scripts/config-schemas.sh
```

## License

[Apache-2.0](LICENSE)
