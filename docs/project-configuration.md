# Project Configuration Guide

This guide provides a comprehensive reference for configuring ICP projects using the `icp.yaml` file. The configuration file defines how your canisters are built, deployed, and managed across different environments.

## Overview

The `icp.yaml` file describes the configuration of your ICP project. It supports:
- Single and multi-canister projects
- Custom build processes and recipes
- Environment-specific deployments
- Network configuration
- Canister settings and metadata

See the schema reference in [./docs/schemas/icp-yaml-schema.json](./docs/schemas/icp-yaml-schema.json)

## Project Lifecycle

When working with ICP projects, your canisters go through several distinct phases during development and deployment:

### Building Canisters

The build phase compiles your source code and generates WASM bytecode files that can run on the Internet Computer. During this phase:

- Build steps defined in your configuration are executed in sequence.
- The result of this step is compiled WASM and possibly some other assets (for eg, static files in the case of an asset canister serving a frontend).
- icp-cli does not concern itself with the compilation process, instead it delegates to the appropriate toolchain.
- Ideally, the WASM and assets produced are reproducible and don't contain any hard coded deployment specific information.
- The toolchains are responsible for detecting whether building is necessary and which version of the compiler to use.

### Creating Canisters

This is a one-time setup phase that occurs automatically when deploying to a network for the first time:

- Empty canister instances are created on the target network.
- Each canister receives a unique canister ID.
- Initial cycles are allocated to cover the canister's operations.
- Canister settings (memory limits, compute allocation, etc.) are applied.

### Deploying Canisters

Deployment updates the WASM code running in your existing canisters:

- The compiled WASM bytecode is installed into the canister.
- Previous WASM code is replaced with the new version.
- Canister state persists across deployments (unless explicitly reset).
- Environment variables and settings are updated if changed.

### Syncing Canisters

The sync phase handles post-deployment operations to synchronize canister state. This is dependent
on the type of canister itself. A typical case is uploading static assets to an asset canister.


These phases work together to provide a complete deployment pipeline from source code to a running canister on the Internet Computer.


## Basic Structure

### Single Canister Project

```yaml
canisters:
  - name: my-canister
    build:
      steps:
        - type: script
          commands:
            - cargo build --target wasm32-unknown-unknown --release
            - mv target/wasm32-unknown-unknown/release/my_canister.wasm "$ICP_WASM_OUTPUT_PATH"
```

### Multi-Canister Project

```yaml
canisters:
  - canisters/*  # Glob pattern
  - path/to/specific/canister.yaml
  - name: inline-canister  # Inline definition
    build:
      steps:
        - type: pre-built
          path: dist/canister.wasm
```

## Configuration Sections

### Canister Configuration

#### Inline Canister Definition

```yaml
canisters:
  - name: my-canister

    # Build configuration (required)
    build:
      steps:
        - type: script
          commands:
            - echo "Building canister..."
            - # Your build commands here

    # Sync configuration (optional)
    sync:
      steps:
        - type: assets
          source: www
          target: /

    # Canister settings (optional)
    settings:
      compute_allocation: 1
      memory_allocation: 4294967296  # 4GB in bytes
      freezing_threshold: 2592000    # 30 days in seconds
      reserved_cycles_limit: 1000000000000
      wasm_memory_limit: 1073741824  # 1GB in bytes
      wasm_memory_threshold: 536870912  # 512MB in bytes
      environment_variables:
        API_URL: "https://api.example.com"
        DEBUG: "false"
```

#### External Canister References

```yaml
canisters:
  - canisters/*              # All YAML files in canisters/ directory
  - frontend/canister.yaml   # Specific file path
  - backend/*.yaml           # Glob pattern for backend canisters
```

### Build Steps

Build steps define how your canister WASM is created. Multiple step types are supported:

#### Script Build Steps

Execute custom shell commands:

```yaml
build:
  steps:
    - type: script
      commands:
        - echo "Starting build..."
        - cargo build --target wasm32-unknown-unknown --release
        - mv target/wasm32-unknown-unknown/release/my_canister.wasm "$ICP_WASM_OUTPUT_PATH"
        - echo "Build complete!"
```

**Environment Variables Available in Scripts:**
- `ICP_WASM_OUTPUT_PATH`: Path where the final WASM should be placed
- `ICP_PROJECT_ROOT`: Root directory of the project
- Standard shell environment variables

#### Pre-built Build Steps

Use an existing WASM file:

```yaml
build:
  steps:
    - type: pre-built
      path: dist/my_canister.wasm
      sha256: a1b2c3...  # Optional integrity check
```

#### Assets Build Steps

Bundle static assets into your canister:

```yaml
build:
  steps:
    - type: assets
      source: www           # Source directory
      target: /             # Target path in canister
      include_patterns:     # Optional: files to include
        - "*.html"
        - "*.css"
        - "*.js"
      exclude_patterns:     # Optional: files to exclude
        - "*.md"
        - "test/**"
```

### Sync Steps

Sync steps handle post-deployment operations, typically for asset canisters:

```yaml
sync:
  steps:
    - type: assets
      source: www
      target: /
      # Upload static files after canister deployment
```

### Recipe System

Recipes provide reusable, templated build configurations. The DFINITY foundation maintains
a set of recipes at https://github.com/dfinity/icp-cli-recipes, you can also host your own
or even refer to a local recipe file.

Recipes are essentially handlebar templates that take a few paramters and render into build+sync steps.

In a manifest they are referred to through the `type` field that can have one of these formats:

#### Local Recipes

A local path to a handlebar template, in this case a file called "myrecipe.hb.yaml"

```yaml
canister:
  name: my-canister
  recipe:
    type: ./myrecipe.hb.yaml
    configuration:  # Configuration passed to the template
      package: my-canister
```

#### Remote Recipes

A URL to a handlebar template.

```yaml
canister:
  name: my-canister
  recipe:
    type: https://recipes.example.com/rust-optimized.hb.yaml
    sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
    configuration:  # Configuration passed to the template
      package: my-canister
```

#### From a registry

Pointing to a known registry. In this particular case, `@dfinity` is mapped to https://github.com/dfinity/icp-cli-recipes
and `prebuilt` points to https://github.com/dfinity/icp-cli-recipes/releases/tag/prebuilt-v2.0.0 

```yaml
canisters:
  - name: my-canister-with-version
    recipe:
      type: "@dfinity/prebuilt@v2.0.0"
      configuration:
        path: ../icp-pre-built/dist/hello_world.wasm
        sha256: d7c1aba0de1d7152897aeca49bd5fe89a174b076a0ee1cc3b9e45fcf6bde71a6
```

### Networks

An ICP network that icp-cli can interact with.
There are two types of networks:
- managed - this is a network whose lifecycle icp-cli is responsible for.
- external - A remote network that is hosted, this could be mainnet or a remote instance of pocket-ic serving as a long lived testnet.

You can define your own networks but there are two implict networks defined:
- local - A managed network that is spun up with the network launcher and typically used for local development.
- mainnet - The mainnet, production network.


```yaml
networks:
  # Inline network definition
  - name: local-testnet
    mode: managed
    gateway:
      host: 127.0.0.1
      port: 4943
    
  # Reference to external network file
  - networks/staging.yaml
  - networks/*.yaml  # All network files in directory
```

#### Network Types

**Managed Networks (Local Development):**
```yaml
name: local-dev
mode: managed
gateway:
  host: 127.0.0.1
  port: 4943
```

**Managed Docker Networks:**
```yaml
name: local-dev
mode: managed
image: ghcr.io/dfinity/icp-cli-network-launcher
port-mapping:
  4943:4943
```
See the [containers docs](./containers.md).

**External Networks:**
```yaml
name: ic-mainnet
mode: external
gateway:
  url: https://ic0.app
```

### Environments

Environments link canisters to networks with specific settings. For example, your project might have a couple of canisters
and you might define 3 different environments:

- local - that you use for development against a managed local network
- ic-stage - A staging environment that is deployed to mainnet
- ic - Your production environment deployed to mainnet

There are implicit environments:
- `local` - Assumes the local network and assumed to be the default
- `ic` - Assumes mainnet

Canisters can have different settings in each environment.

Example configuration:

```yaml
environments:
  - name: development
    network: local-dev
    canisters:
      - frontend
      - backend
    settings:
      frontend:
        memory_allocation: 1073741824  # 1GB
      backend:
        compute_allocation: 5
        environment_variables:
          MY_ENV_VAR: "some value"
  
  - name: production
    network: ic-mainnet
    canisters:
      - frontend
      - backend
    settings:
      frontend:
        memory_allocation: 4294967296  # 4GB
        freezing_threshold: 7776000    # 90 days
      backend:
        compute_allocation: 10
        reserved_cycles_limit: 10000000000000
        environment_variables:
          MY_ENV_VAR: "some value"
```

## Canister Settings Reference

### Compute and Memory Allocation

```yaml
settings:
  # Compute allocation (0-100): Guaranteed compute capacity percentage
  compute_allocation: 5
  
  # Memory allocation in bytes: Fixed memory reservation
  # Dynamic allocation is used if memory_allocation is not set
  memory_allocation: 4294967296  # 4GB
  
  # Freezing threshold in seconds: Time before canister freezes due to low cycles
  freezing_threshold: 2592000  # 30 days
  
  # Reserved cycles limit: Maximum cycles the canister can consume
  reserved_cycles_limit: 1000000000000  # 1T cycles

  # WASM memory limit in bytes: Maximum heap size for WASM module
  wasm_memory_limit: 1073741824  # 1GB
  
  # WASM memory threshold in bytes: Triggers low-memory callback
  wasm_memory_threshold: 536870912  # 512MB
```

### Canister Environment Variables

Canister Environment Variables are variables available to your canister at *runtime*.
They allow compiling once and running the same WASM with different configuration.

```yaml
settings:
  environment_variables:
    NODE_ENV: "production"
    API_URL: "https://api.example.com"
    FEATURE_FLAGS: "advanced_mode=true,beta_features=false"
    CORS_ORIGINS: "https://myapp.com,https://staging.myapp.com"
```

## Advanced Configuration Patterns

### Multi-Environment Setup

```yaml
# Root icp.yaml
canisters:
  - frontend/canister.yaml
  - backend/canister.yaml

networks:
  - name: local
    mode: managed
  - name: testnet
    mode: external
    gateway:
      url: https://testnet.ic0.app
  - name: mainnet  
    mode: external
    gateway:
      url: https://ic0.app

environments:
  - name: dev
    network: local
    canisters: [frontend, backend]
    
  - name: test
    network: testnet
    canisters: [frontend, backend]
    settings:
      frontend:
        memory_allocation: 2147483648  # 2GB
      
  - name: prod
    network: mainnet
    canisters: [frontend, backend]
    settings:
      frontend:
        memory_allocation: 4294967296  # 4GB
        compute_allocation: 10
      backend:
        compute_allocation: 20
        reserved_cycles_limit: 50000000000000
```

## Migration from dfx.json

TODO

