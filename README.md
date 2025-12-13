# icp-cli

A command-line interface for developing and deploying applications on the Internet Computer Protocol (ICP).

## Usage

See the [command line reference](docs/cli-reference.md).

## Installing

For now, you have to build icp-cli locally in order to use it.

### Prerequisites

- **Rust**: Install Rust using [rustup](https://rustup.rs/). The project uses Rust 2024 edition.
- **Network launcher**: Download [icp-cli-network-launcher](https://github.com/dfinity/icp-cli-network-launcher/releases) in order to run a local network.
- **mops**: Required if you want to build Motoko canister. See [mops.one](https://cli.mops.one/).

### Building

```bash
# Build all crates in the workspace
cargo build

# Add target directory to your path
export PATH=$(pwd)/target/debug:$PATH

# Check that you can run
icp help
```

### Add network launcher and motoko tools to the path

To launch a local network you will also need to set the `ICP_CLI_NETWORK_LAUNCHER_PATH` environment variable.
You can download the correct version for your machine from [github](https://github.com/dfinity/icp-cli-network-launcher/releases).

```bash
# for eg for a mac with apple sillicon:
wget https://github.com/dfinity/icp-cli-network-launcher/releases/download/v11.0.0/icp-cli-network-launcher-arm64-darwin-v11.0.0.tar.gz -O icp-cli-network-launcher-arm64-darwin-v11.0.0.tar.gz
gunzip icp-cli-network-launcher-arm64-darwin-v11.0.0.tar.gz
export ICP_CLI_NETWORK_LAUNCHER_PATH="$(pwd)/icp-cli-network-launcher-arm64-darwin-v11.0.0/icp-cli-network-launcher"
```

Note: The network launcher binary must not end up in a different directory than the pocket-ic binary.

### [Optional] Add motoko tools to the path

You might also need the Motoko compiler if you plan on building canisters with Motoko. The best way
is to install the mops, the motoko package manager, see: https://cli.mops.one/

Reminder, when mops is installed the first time, you must initialize the toolchain with:

```bash
mops toolchain init
```

### Examples

The `examples/` directory contains various project templates and configurations that demonstrate how to use the CLI with different project types:

- `icp-motoko/` - Motoko canister example
- `icp-rust/` - Rust canister example  
- `icp-static-assets/` - Static website deployment
- `icp-multi-canister/` - Multi-canister project setup
- And many more...

## Development

### Prerequisites

- **Rust**: Install Rust using [rustup](https://rustup.rs/). The project uses Rust 2024 edition.

### Building

This is a Rust workspace with multiple crates. To build the project:

```bash
# Build all crates in the workspace
cargo build

# Build in release mode for better performance
cargo build --release

# Build only the CLI binary
cargo build --bin icp
```

The compiled binary will be available at `target/debug/icp` (or `target/release/icp` for release builds).

### Running Tests

#### Prerequisites for Testing

The tests require the network launcher for running the local network. See the setup instructions below.

#### Setup

The `ICP_CLI_NETWORK_LAUNCHER_PATH` environment variable should point to
the path of the `icp-cli-network-launcher` binary.

You can download the correct version for your machine from [github](https://github.com/dfinity/icp-cli-network-launcher/releases/tag/v11.0.0).

To run the tests, it's necessary to set the `ICP_CLI_NETWORK_LAUNCHER_PATH` environment variable.
Here is one way to do that:

```
# Export the path to the network launcher binary
export ICP_CLI_NETWORK_LAUNCHER_PATH="<yourpath>/icp-cli-network-launcher"

# Run tests
cargo test
```

### Generating CLI Documentation

The project includes automatic CLI documentation generation using `clap_markdown`. To generate comprehensive documentation for all commands:

```bash
# Run the documentation generation script
./scripts/generate-cli-docs.sh
```

This will:
- Build the CLI in release mode
- Generate complete markdown documentation at `docs/cli-reference.md`

You can also generate documentation manually:

```bash
# Build the CLI first
cargo build --release

# Generate markdown documentation
./target/release/icp --markdown-help > docs/cli-reference.md
```

## Contributing

Contributions are welcome! Please see the [contribution guide](./.github/CONTRIBUTING.md) for more information.

## License

This project is licensed under the [Apache-2.0](./LICENSE) license.
