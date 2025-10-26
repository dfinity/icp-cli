# Project Configuration Guide

This guide provides a comprehensive reference for configuring ICP projects using the `icp.yaml` file. The configuration file defines how your canisters are built, deployed, and managed across different environments.

## Overview

The `icp.yaml` file describes the configuration of your ICP project. It supports:
- Single and multi-canister projects
- Custom build processes and recipes
- Environment-specific deployments
- Network configuration
- Canister settings and metadata

See the schema reference in [./docs/schemas/canister-yaml-schema.json](./docs/schemas/canister-yaml-schema.json)

## Project Lifecycle

When working with ICP projects, your canisters go through several distinct phases during development and deployment:

### Building Canisters

The build phase compiles your source code and generates WASM bytecode files that can run on the Internet Computer. During this phase:

- Build steps defined in your configuration are executed in sequence.
- The result of this step is compiled WASM and possibly some other assets (for eg, static files in the case of an asset canister serving a frontend).
- icp-cli does not concern itself with the compilation process, instead it delegates to the appropriate toolchain.
- Ideally, the WASM and assets produced are reproducible and don't contain any hard coded deployment specific information.
- The toolchains are responsible for detecting whether building is necessary.

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
canister:
  name: my-canister
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
canister:
  name: my-canister
  
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

Recipes provide reusable, templated build configurations. At the moment there are
different types of recipes you can use:
- built-in recipes - Those are hard coded into `icp-cli` itself.
- local recipes - Those are handlebar templates defined locally in your project.
- remote recipes - Those are handlebar templates defined in a remote location that
can be reused and shared across teams or with the community.

#### Built-in Recipes

Those recipes are baked into `icp-cli`, they might be impacted by upgrades to `icp-cli`
and require a new releast to be modified.

```yaml
canister:
  name: my-canister
  recipe:
    type: rust  # Built-in Rust recipe
    configuration:
      package: my-canister  # Cargo package name
```

**Available Built-in Recipes:**
- `assets`: Asset canister
- `motoko`: Motoko compiler integration
- `rust`: Cargo-based Rust canister builds

#### Custom Local Recipes

```yaml
canister:
  name: my-canister
  recipe:
    type: file://./recipes/custom-build.hb.yaml
    configuration:  # Configuration passed to the template
      source_dir: src
      optimization_level: 3
```

#### Remote Recipes

```yaml
canister:
  name: my-canister
  recipe:
    type: https://recipes.example.com/rust-optimized.hb.yaml
    sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
    configuration:  # Configuration passed to the template
      package: my-canister
```

### Networks

Define custom networks for deployment:

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

**External Networks:**
```yaml
name: ic-mainnet
mode: external
gateway:
  url: https://ic0.app
```

### Environments

Environments link canisters to networks with specific settings:

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

### Environment Variables

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

