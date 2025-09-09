# Project Configuration Guide

This guide provides a comprehensive reference for configuring ICP projects using the `icp.yaml` file. The configuration file defines how your canisters are built, deployed, and managed across different environments.

## Overview

The `icp.yaml` file is the central configuration for your ICP project. It supports:
- Single and multi-canister projects
- Custom build processes and recipes
- Environment-specific deployments
- Network configuration
- Canister settings and metadata

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

Recipes provide reusable, templated build configurations:

#### Built-in Recipes

```yaml
canister:
  name: my-canister
  recipe:
    type: rust  # Built-in Rust recipe
    configuration:
      package: my-canister  # Cargo package name
```

**Available Built-in Recipes:**
- `rust`: Cargo-based Rust canister builds
- `motoko`: Motoko compiler integration
- (More recipes available - check documentation)

#### Custom Local Recipes

```yaml
canister:
  name: my-canister
  recipe:
    type: file://./recipes/custom-build.hb.yaml
    configuration:
      source_dir: src
      optimization_level: 3
```

#### Remote Recipes

```yaml
canister:
  name: my-canister
  recipe:
    type: https://recipes.example.com/rust-optimized.hb.yaml
    configuration:
      package: my-canister
      features: ["production"]
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
          NODE_ENV: "development"
  
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
          NODE_ENV: "production"
          API_RATE_LIMIT: "1000"
```

## Canister Settings Reference

### Compute and Memory Allocation

```yaml
settings:
  # Compute allocation (0-100): Guaranteed compute capacity percentage
  compute_allocation: 5
  
  # Memory allocation in bytes: Fixed memory reservation
  memory_allocation: 4294967296  # 4GB
  
  # Dynamic allocation is used if memory_allocation is not set
```

### Lifecycle Management

```yaml
settings:
  # Freezing threshold in seconds: Time before canister freezes due to low cycles
  freezing_threshold: 2592000  # 30 days
  
  # Reserved cycles limit: Maximum cycles the canister can consume
  reserved_cycles_limit: 1000000000000  # 1T cycles
```

### WASM Configuration

```yaml
settings:
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

### Conditional Builds with Recipes

```yaml
canister:
  name: my-canister
  recipe:
    type: rust
    configuration:
      package: my-canister
      features:
        - "{{ environment == 'production' ? 'optimized' : 'debug' }}"
      profile: "{{ environment == 'production' ? 'release' : 'dev' }}"
```

### Asset Canister with Build Pipeline

```yaml
canister:
  name: frontend
  build:
    steps:
      # Build the web application
      - type: script
        commands:
          - npm ci
          - npm run build
      
      # Bundle assets into canister
      - type: assets
        source: dist
        target: /
        exclude_patterns:
          - "*.map"
          - "test/**"
  
  sync:
    steps:
      # Upload assets after deployment
      - type: assets
        source: dist
        target: /
```

## Configuration Validation

ICP CLI validates your configuration and provides helpful error messages:

```bash
# Validate configuration without building
icp build --dry-run

# Check specific canister configuration  
icp build my-canister --dry-run
```

## Best Practices

### Organization
- Use external canister files for complex multi-canister projects
- Group related canisters in subdirectories
- Use consistent naming conventions

### Security
- Set appropriate freezing thresholds for production canisters
- Use reserved cycles limits to prevent runaway consumption
- Validate pre-built WASM files with SHA256 hashes

### Performance
- Configure memory allocation based on actual usage patterns
- Use compute allocation for guaranteed performance in production
- Monitor WASM memory usage and set appropriate limits

### Development Workflow
- Use environments for different deployment stages
- Leverage recipes for consistent build processes
- Version control your configuration alongside your code

## Troubleshooting Configuration

### Common Issues

**Invalid YAML syntax**
```bash
Error: Configuration parsing failed
Help: Check YAML syntax, proper indentation, and quote strings containing special characters
```

**Missing build steps**
```bash
Error: Canister 'my-canister' has no build steps defined
Help: Add a build section with at least one build step
```

**Invalid glob patterns**
```bash
Error: No canisters found matching pattern 'canisters/*'
Help: Verify the directory exists and contains canister configuration files
```

**Recipe resolution failures**
```bash
Error: Failed to resolve recipe 'https://example.com/recipe.yaml'
Help: Check network connectivity and recipe URL validity
```

### Debugging Tips

1. Use `--debug` flag for verbose logging
2. Validate configuration with `--dry-run` before building
3. Check file paths and glob patterns carefully
4. Ensure all referenced files and directories exist
5. Verify network connectivity for remote recipes

## Migration from dfx.json

If you're migrating from dfx, here's a rough mapping:

| dfx.json | icp.yaml |
|----------|----------|
| `canisters.*.type` | `build.steps[].type` |
| `canisters.*.build` | `build.steps[].commands` |
| `networks.*` | `networks[].config` |
| `defaults.build.packtool` | Recipe configuration |
| `defaults.replica.*` | Network configuration |

For detailed migration assistance, see our [migration guide](migration-from-dfx.md).
