# icp-cli

A command-line tool for building and deploying applications on the [Internet Computer](https://internetcomputer.org).

## Quick Start

**Prerequisites:** [Node.js](https://nodejs.org/) (LTS)

**Install:**

```bash
# icp-cli and ic-wasm (required)
npm install -g @icp-sdk/icp-cli @icp-sdk/ic-wasm

# Motoko toolchain (for Motoko projects)
npm install -g ic-mops
```

> **Alternative methods:** See the [Installation Guide](docs/guides/installation.md) for Homebrew, shell script, Rust setup, or platform-specific instructions.

Then follow the **[Quickstart](docs/quickstart.md)** to deploy your first canister in under 5 minutes.

## For dfx Users

If you're coming from dfx (the previous Internet Computer SDK), see the **[Migration Guide](docs/migration/from-dfx.md)** for command mappings, workflow differences, and how to migrate existing projects.

## Documentation

📚 **[Full Documentation Site](https://dfinity.github.io/icp-cli/)** — Complete guides, tutorials, and reference

Or browse the markdown docs directly:

- **[Quickstart](docs/quickstart.md)** — Deploy a canister in under 5 minutes
- **[Tutorial](docs/tutorial.md)** — Learn icp-cli step by step
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

## Getting Help

- **[Documentation](docs/index.md)** — Guides, concepts, and reference
- **[GitHub Issues](https://github.com/dfinity/icp-cli/issues)** — Bug reports and feature requests
- **[Developer Forum](https://forum.dfinity.org/)** — Questions and discussions
- **[Discord](https://discord.internetcomputer.org)** — Real-time community chat in #dx-feedback

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](.github/CONTRIBUTING.md) for detailed guidelines.

### Prerequisites

- Rust 1.88.0+ ([rustup.rs](https://rustup.rs/))

| Platform      | Install                                                                                                  |
|---------------|----------------------------------------------------------------------------------------------------------|
| macOS         | `xcode-select --install`                                                                                 |
| Ubuntu/Debian | `sudo apt install build-essential pkg-config libssl-dev`                                                 |
| Fedora/RHEL   | `sudo dnf install gcc pkg-config openssl-devel`                                                          |
| Arch Linux    | `sudo pacman -S base-devel openssl`                                                                      |
| Windows       | VS build tools (see [Rustup's guide](https://rust-lang.github.io/rustup/installation/windows-msvc.html)) |

Tests additionally depend on `wasm-tools`, `mitmproxy`, and SoftHSM2. 

### Build and Test

```bash
git clone https://github.com/dfinity/icp-cli.git
cd icp-cli
cargo build
cargo test
```

### Development

```bash
# Run the CLI during development
cargo run -- <command>

# Build release binary
cargo build --release
# Binary is at target/release/icp

# Format and lint
cargo fmt && cargo clippy

# Generate CLI docs (after changing commands)
./scripts/generate-cli-docs.sh

# Update config schemas (after changing manifest types)
./scripts/generate-config-schemas.sh
```

### Working with Documentation

```bash
# Preview documentation site locally
cd docs-site && npm install && npm run dev
# Opens at http://localhost:4321

# Prepare docs for build (runs automatically during build)
./scripts/prepare-docs.sh
```

Documentation structure follows the [Diátaxis framework](https://diataxis.fr/):
- [`docs/quickstart.md`](docs/quickstart.md) - Deploy in under 5 minutes
- [`docs/tutorial.md`](docs/tutorial.md) - Learn step by step
- [`docs/guides/`](docs/guides/index.md) - Task-oriented how-to guides
- [`docs/concepts/`](docs/concepts/index.md) - Understanding-oriented explanations
- [`docs/reference/`](docs/reference/index.md) - Information-oriented specifications
- [`docs/migration/`](docs/migration/from-dfx.md) - Migration guides

See [docs/README.md](docs/README.md) for documentation writing guidelines.

## License

[Apache-2.0](LICENSE)
