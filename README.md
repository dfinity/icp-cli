# icp-cli

A command-line interface for developing and deploying applications on the Internet Computer Protocol (ICP).

## Usage

See the [command line reference](docs/cli-reference.md).

## Installing

For now, you have to build icp-cli locally in order to use it.

### Prerequisites

- **Rust**: Install Rust using [rustup](https://rustup.rs/). The project uses Rust 2024 edition.
- **dfx**: Install the [DFINITY SDK](https://internetcomputer.org/docs/building-apps/getting-started/install) for IC development.

### Building

```bash
# Build all crates in the workspace
cargo build

# Add target directory to your path
export PATH=$(pwd)/target/debug:$PATH

# Check that you can run
icp help
```

### Add pocket-ic and motoko tools to the path

To launch a local network you will also need to have pocket-ic on the path. You might
also need the Motoko compiler if you plan on building canisters with Motoko. For now,
a good way to do this is to use the tools that ship with `dfx`. One way to configure them
is to run the following in your terminal:

```bash
# Ensure dfx is installed and the cache is populated
dfx cache install

# Export the path to the pocket-ic binary
export ICP_POCKET_IC_PATH="$(dfx cache show)/pocket-ic"

# Add moc to the path
export PATH=$(dfx cache show):$PATH
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
- **dfx**: Install the [DFINITY SDK](https://internetcomputer.org/docs/building-apps/getting-started/install) for IC development.

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

These tests use dfx to stand up and interact with a local Internet Computer instance.
To ensure test isolation, they run in a temporary HOME directory and 
**cannot use the dfx shim from dfxvm**.

#### Setup

The `ICP_POCKET_IC_PATH` environment variable should point to
the path of the `pocket-ic` binary.

To run the tests, it's necessary to set the `ICPTEST_DFX_PATH` environment variable
to a valid dfx path, as well as the `ICP_POCKET_IC_PATH` environment variable.
Here is one way to do that:

```
# Ensure dfx is installed and the cache is populated
dfx cache install

# Export the path to the actual dfx binary (not the shim)
export ICPTEST_DFX_PATH="$(dfx cache show)/dfx"

# Export the path to the pocket-ic binary
export ICP_POCKET_IC_PATH="$(dfx cache show)/pocket-ic"

# Run tests
cargo test
```

If ICPTEST_DFX_PATH is not set, tests that depend on dfx will fail.

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

