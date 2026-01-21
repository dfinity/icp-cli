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

# Show the status of your canisters
icp canister status

# Call a function on your canister
# icp canister call <canister-name> greet '("World")'
# The ones generated from the templates are typically called `backend`

icp canister call backend greet '("World")'
```

See the [Installation Guide](docs/guides/installation.md) for all installation methods including building from source.

## For dfx Users

If you're coming from dfx (the previous Internet Computer SDK), see the **[Migration Guide](docs/migration/from-dfx.md)** for command mappings, workflow differences, and how to migrate existing projects.

## Documentation

📚 **[Full Documentation Site](https://dfinity.github.io/icp-cli/)** — Complete guides, tutorials, and reference

Or browse the markdown docs directly:

- **[Tutorial](docs/tutorial.md)** — Deploy your first canister
- **[Guides](docs/guides/)** — How to accomplish common tasks
- **[Concepts](docs/concepts/)** — Understand how icp-cli works
- **[Reference](docs/reference/cli.md)** — Complete CLI and configuration reference

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

Contributions are welcome! See [CONTRIBUTING.md](.github/CONTRIBUTING.md) for detailed guidelines.

### Development Quick Start

```bash
# Build the project
cargo build

# Run tests
cargo test

# Format and lint
cargo fmt && cargo clippy
```

### Working with Documentation

```bash
# Preview documentation site locally
cd docs-site && npm install && npm run dev
# Opens at http://localhost:4321

# Generate CLI reference (when commands change)
./scripts/generate-cli-docs.sh

# Generate config schemas (when manifest types change)
./scripts/generate-config-schemas.sh

# Prepare docs for build (runs automatically during build)
./scripts/prepare-docs.sh
```

Documentation structure follows the [Diátaxis framework](https://diataxis.fr/):
- [`docs/guides/`](docs/guides/) - Task-oriented how-to guides
- [`docs/concepts/`](docs/concepts/) - Understanding-oriented explanations
- [`docs/reference/`](docs/reference/) - Information-oriented specifications
- [`docs/migration/`](docs/migration/) - Migration guides

See [docs/README.md](docs/README.md) for documentation writing guidelines.

## License

[Apache-2.0](LICENSE)
