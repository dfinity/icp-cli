# Getting Started with ICP CLI

This guide will walk you through setting up ICP CLI and deploying your first canister to the Internet Computer.

## What is ICP CLI?

ICP CLI is a modern command-line tool for developing and deploying applications on the Internet Computer Protocol (ICP). It provides a streamlined workflow for building, testing, and deploying canisters with support for multiple programming languages and deployment environments.

## Prerequisites

Before you begin, ensure you have the following installed:

### Required
- **Rust**: Install using [rustup](https://rustup.rs/)
- **dfx**: Install the [DFINITY SDK](https://internetcomputer.org/docs/building-apps/getting-started/install)

### Language-Specific Requirements
- **For Rust canisters**: `rustup target add wasm32-unknown-unknown`
- **For Motoko canisters**: Motoko compiler (`moc`) - included with dfx

## Installation

Currently, you need to build ICP CLI from source:

```bash
# Clone the repository (if not already done)
git clone <repository-url>
cd icp-cli

# Build the project
cargo build --release

# Add to PATH for easier access
export PATH=$(pwd)/target/release:$PATH

# Verify installation
icp help
```

### Setting Up Dependencies

Configure the tools that ICP CLI depends on:

```bash
# Ensure dfx is installed and cache is populated
dfx cache install

# Export path to pocket-ic for local network support
export ICP_POCKET_IC_PATH="$(dfx cache show)/pocket-ic"

# Add Motoko compiler to PATH (if building Motoko canisters)
export PATH=$(dfx cache show):$PATH
```

## Your First Canister

Let's create and deploy a simple "Hello World" canister.

### 1. Create a New Project

Choose from one of the examples to get started quickly:

```bash
# Copy a template (choose one)
cp -r examples/icp-motoko my-first-project     # For Motoko
cp -r examples/icp-rust my-first-project       # For Rust

cd my-first-project
```

### 2. Understand the Project Structure

Your project contains:

- `icp.yaml` - Project configuration file
- `src/` - Source code directory
- `README.md` - Project-specific instructions

Let's look at the `icp.yaml` file:

```yaml
canister:
  name: my-canister
  build:
    steps:
      - type: script
        commands:
          # Build commands specific to your language
```

### 3. Start a Local Network

In a separate terminal, start the local Internet Computer network:

```bash
icp network run
```

This starts a local replica where you can deploy and test your canisters. Keep this running throughout development.

### 4. Build Your Canister

Build the canister from your source code:

```bash
icp build
```

This command:
- Executes the build steps defined in `icp.yaml`
- Compiles your source code to WebAssembly (WASM)
- Prepares the canister for deployment

### 5. Deploy to Local Network

Deploy your canister to the local network:

```bash
icp deploy
```

This command:
- Creates a new canister ID (if first deployment)
- Installs the WASM code to the canister
- Makes your canister available for interaction

### 6. Interact with Your Canister

Call methods on your deployed canister:

```bash
# For the example canisters, try:
icp canister call my-canister greet '("World")'
```

You should see a response like `("Hello, World!")`.

## Common Workflows

### Development Cycle
```bash
# 1. Make changes to your source code
# 2. Build the updated canister
icp build

# 3. Redeploy (upgrade) the canister
icp deploy --mode upgrade

# 4. Test your changes
icp canister call my-canister method_name '(args)'
```

### Working with Multiple Canisters
```bash
# Build specific canisters
icp build canister1 canister2

# Deploy specific canisters
icp deploy canister1

# List all canisters in project
icp canister list
```

### Environment Management
```bash
# Deploy to Internet Computer mainnet
icp deploy --ic

# Deploy to a custom environment
icp deploy --environment staging
```

## Project Configuration Basics

The `icp.yaml` file is the heart of your project configuration. Here are the key concepts:

### Single Canister Project
```yaml
canister:
  name: my-canister
  build:
    steps:
      - type: script
        commands:
          - cargo build --target wasm32-unknown-unknown --release
```

### Multi-Canister Project
```yaml
canisters:
  - canisters/*  # Glob pattern to find canister configs
```

### Using Recipes
```yaml
canister:
  name: my-canister
  recipe:
    type: rust  # Built-in recipe for Rust canisters
    configuration:
      package: my-canister
```

## Next Steps

Now that you have your first canister running, explore:

1. **[Project Configuration](project-configuration.md)** - Deep dive into `icp.yaml` options
2. **[CLI Reference](cli-reference.md)** - Complete command documentation  
3. **[Examples](../examples/)** - More complex project templates
4. **[Advanced Workflows](workflows.md)** - Multi-environment deployments, CI/CD

## Troubleshooting

### Common Issues

**Build fails with "command not found"**
- Ensure all required tools are installed and in PATH
- Check language-specific prerequisites

**Network connection fails**
- Verify `icp network run` is running in another terminal
- Check that `ICP_POCKET_IC_PATH` is set correctly

**Permission errors**
- Ensure you have write permissions in the project directory
- Check that temporary directories aren't protected

**Canister deployment fails**
- Verify the local network is healthy: `icp network ping --wait-healthy`
- Check canister build succeeded: `icp build`

### Getting Help

- Use `icp help` for command overview
- Use `icp <command> --help` for specific command help
- Check the [examples](../examples/) directory for reference implementations

## What's Different from dfx?

If you're familiar with dfx, here are the key differences:

- **Unified configuration**: Single `icp.yaml` vs multiple config files
- **Recipe system**: Reusable build templates and remote configurations
- **Environment management**: Built-in support for multiple deployment targets
- **Modern CLI**: Improved UX with better error messages and progress indicators

Ready to build more complex applications? Check out our [examples](../examples/) or dive into [project configuration](project-configuration.md)!
