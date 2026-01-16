# Getting Started with ICP CLI

This guide will walk you through setting up ICP CLI and deploying your first canister to the Internet Computer.

## What is ICP CLI?

ICP CLI is a modern command-line tool for developing and deploying applications on the Internet Computer Protocol (ICP). It provides a streamlined workflow for building, testing, and deploying canisters with support for multiple programming languages and deployment environments.

## Prerequisites

Before you begin, ensure you have the following installed:

### Required
- **Rust**: Install using [rustup](https://rustup.rs/)

### Language-Specific Requirements
- **For Rust canisters**: `rustup target add wasm32-unknown-unknown`
- **For Motoko canisters**: Motoko compiler (`moc`) - included with [dfx](https://internetcomputer.org/docs/building-apps/getting-started/install)

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

If you want to use Motoko, install the Motoko package manager:

```bash
# Make sure you have installed mops, see https://cli.mops.one/
# and have initialized the toolchain
mops toolchain init
```

## Your First Canister

Let's create and deploy a simple "Hello World" canister.

### 1. Create a New Project

Choose from one of the examples to get started quickly:

```bash
# Create a project from a template
# by default icp-cli will use templates in https://github.com/dfinity/icp-cli-templates
#
# 
icp new my-project

#
# Select the type of project you want to start with
#

cd my-project
```

### 2. Understand the Project Structure

A typical project contains:

- `icp.yaml` - Project configuration file
- `src/` - Source code directory
- `README.md` - Project-specific instructions

Let's look at the `icp.yaml` file:

```yaml
canisters:
  - name: my-canister
    build:
      steps:
        - type: script
          commands:
            # Build commands specific to your language
```

Note that the configuration can be split across different files. To see the effective
project configuration, you can run:

```bash
icp project show
```

### 3. Start a Local Network

Start the local Internet Computer network:

```bash
icp network start -d
```

This starts a local replica where you can deploy and test your canisters.

### 4. Build Your Canister

Build your project:

```bash
icp build my-canister
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
icp build my-canister

# 3. Redeploy (upgrade) the canister
icp deploy

# 4. Test your changes
icp canister call my-canister method_name '(args)'
```

### Working with Multiple Canisters
```bash
# Build a specific canister
icp build canister1

# Deploy specific canisters
icp deploy canister1

# List all canisters in project
icp canister list
```

## Networks and Environments

A *network* is a url through which you can reach an ICP network. This could be "ic",
the official ICP network reachable at https://icp-api.io, a local or remote network started
for test or development purposes.

An *environment* represents a set of canisters to deploy to a network. For example you could
have:
- A local development environment pointing using your local network
- A staging environment deployed to the IC mainnet
- A production envrionment deployed to the IC mainnet

For example:

```yaml
environments:
  - name: staging
    network: ic
  - name: prod
    network: ic
```
There is always an implicit "local" environment using the "local" network which is the default
and that cannot be overriden.

To deploy to a specific environment use:

```bash

# Deploy to a custom environment
icp deploy --environment staging
```

## Project Configuration Basics

The `icp.yaml` file is the heart of your project configuration. Here are the key concepts:

### Single Canister Project
```yaml
canisters:
  - name: my-canister
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

Recipes allow templating build instructions and sharing them across projects.
The DFINITY foundation maintains a set of recipes at https://github.com/dfinity/icp-cli-recipes.
You can also host your own.

```yaml
canisters:
  - name: my-canister
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
- Verify `icp network start` is running in another terminal
- The network launcher is automatically downloaded on first use. If you experience issues, you can manually set `ICP_CLI_NETWORK_LAUNCHER_PATH` to a specific launcher binary for debugging

**Canister deployment fails**
- Verify that the local network is healthy: `icp network ping`
- Check canister build succeeded: `icp build <canister-name>`

### Getting Help

- Use `icp help` for command overview
- Use `icp <command> --help` for specific command help
- Check the [examples](../examples/) directory for reference implementations

## What's Different from dfx?

If you're familiar with dfx, here are the key differences:

- **Configuration**: Project configuration is in `icp.yaml` vs `dfx.json`.
- **Environment**: A project is deployed to an "environment" not a network. An environment
is a logical name that points to a network (could be the IC mainnet or your local network).
- **Recipe system**: Reusable build templates you can share with your team or the community.
- **Consistent with mainnet**: Aims to make interacting with the local network the same as interacting
with the IC mainnet.

Ready to build more complex applications? Check out our [examples](../examples/) or dive into [project configuration](project-configuration.md)!
