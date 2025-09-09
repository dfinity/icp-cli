# ICP Project Configuration Schema (`icp.yaml`)

This document provides a comprehensive reference for the `icp.yaml` file format used by the ICP CLI to configure projects, canisters, and deployments.

## Schema Generation

The JSON Schema for `icp.yaml` files is automatically generated from the Rust type definitions in the codebase using the [`schemars`](https://docs.rs/schemars/) crate. This ensures the schema always stays in sync with the actual implementation.

### Regenerating the Schema

To regenerate the JSON Schema after making changes to the configuration types:

```bash
# Using the convenience script
./scripts/generate-schema.sh

# Or manually
cargo run --bin schema-gen
```

The generated schema file is `icp-yaml-schema.json` and can be used for:
- IDE validation and autocomplete
- CI/CD validation of configuration files  
- Documentation generation
- Integration with other tooling

## Overview

The `icp.yaml` file is the central configuration for your ICP project. It defines:

- **Canisters**: How your canisters are built and configured
- **Networks**: Where your canisters are deployed
- **Environments**: Different deployment stages (dev, staging, production)
- **Build processes**: How source code is compiled to WebAssembly
- **Sync operations**: How assets and data are synchronized post-deployment

## Root Schema

```yaml
# Single canister project
canister:
  name: my-canister
  # ... canister configuration

# OR multi-canister project
canisters:
  - canisters/*  # Glob patterns
  - path/to/canister.yaml  # Explicit paths
  - name: inline-canister  # Inline definitions
    # ... canister configuration

# Optional network definitions
networks:
  - networks/*  # Glob patterns
  - name: staging  # Inline definitions
    # ... network configuration

# Optional environment definitions  
environments:
  - name: production
    # ... environment configuration
```

## Canister Configuration

### Basic Structure

```yaml
canister:
  name: my-canister           # Required: unique canister name
  settings:                   # Optional: runtime settings
    # ... settings
  build:                      # Required: build configuration
    steps:
      # ... build steps
  sync:                       # Optional: sync configuration
    steps:
      # ... sync steps
```

### Canister Settings

Runtime configuration applied when the canister is created or updated:

```yaml
settings:
  # Compute allocation (0-100): guaranteed compute capacity percentage
  compute_allocation: 1
  
  # Memory allocation in bytes: if unset, memory is dynamic
  memory_allocation: 4294967296  # 4GB
  
  # Freezing threshold in seconds: inactivity period before freezing
  freezing_threshold: 2592000    # 30 days
  
  # Reserved cycles limit: maximum cycles the canister can consume  
  reserved_cycles_limit: 1000000000000
  
  # WASM memory limit in bytes: upper bound for heap growth
  wasm_memory_limit: 1073741824  # 1GB
  
  # WASM memory threshold in bytes: triggers callback when exceeded
  wasm_memory_threshold: 536870912  # 512MB
  
  # Environment variables accessible within the canister
  environment_variables:
    API_URL: "https://api.example.com"
    DEBUG: "false"
    NODE_ENV: "production"
```

## Build Configuration

### Build Steps

Build steps define how your canister source code is compiled into WebAssembly:

```yaml
build:
  steps:
    # Script adapter: run custom commands
    - type: script
      command: "cargo build --target wasm32-unknown-unknown --release"
      
    # OR multiple commands
    - type: script
      commands:
        - "npm ci"
        - "npm run build"
        - "mv dist/app.wasm $ICP_WASM_OUTPUT_PATH"
    
    # Pre-built adapter: use existing WASM file
    - type: pre-built
      path: "dist/canister.wasm"          # Local file
      sha256: "17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a"
      
    # OR remote WASM file  
    - type: pre-built
      url: "https://github.com/example/releases/latest/canister.wasm"
      sha256: "17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a"
```

### Environment Variables in Build

Build scripts have access to special environment variables:

- `$ICP_WASM_OUTPUT_PATH`: Path where the final WASM file should be written

## Sync Configuration

Sync steps run after deployment to synchronize additional data:

```yaml
sync:
  steps:
    # Assets sync: upload files to assets canister
    - type: assets
      dir: "www"                     # Single directory
      
    # OR multiple directories
    - type: assets
      dirs: ["www", "assets", "public"]
    
    # Script sync: run custom sync commands
    - type: script
      command: "echo 'Canister deployed successfully'"
      
    # OR multiple commands
    - type: script
      commands:
        - "npm run post-deploy"
        - "echo 'Sync complete'"
```

## Recipe Configuration

Recipes provide reusable build templates with parameterization:

```yaml
canister:
  name: my-canister
  recipe:
    type: rust                    # Built-in recipe types: rust, motoko, assets
    configuration:
      package: my-canister
      features: ["optimized"]
      profile: "release"
      
  # OR custom recipe from file
  recipe:
    type: file://recipe.hb.yaml   # Local Handlebars template
    configuration:
      path: "dist/app.wasm"
      sha256: "17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a"
      
  # OR remote recipe
  recipe:
    type: https://example.com/recipes/rust.yaml
    configuration:
      package: my-canister
```

## Network Configuration

### Managed Networks

Networks that the CLI starts and manages locally:

```yaml
networks:
  - name: local
    mode: managed
    gateway:
      host: 127.0.0.1       # Gateway host (default: 127.0.0.1)
      port: 8000            # Gateway port (default: 8000, 0 = random)
      
  - name: test
    mode: managed  
    gateway:
      host: 0.0.0.0         # Listen on all interfaces
      port: 0               # Use random available port
```

### Connected Networks

External networks that you connect to but don't manage:

```yaml
networks:
  - name: ic
    mode: connected
    url: https://icp0.io              # Required: network URL
    root_key: "308182301d060d2b..."   # Optional: network root key
    
  - name: testnet
    mode: connected
    url: https://testnet.dfinity.network
```

## Environment Configuration

Environments define deployment targets with specific network and canister configurations:

```yaml
environments:
  - name: local
    network: local              # Target network
    canisters: ["frontend"]     # Canisters to deploy
    settings:                   # Per-canister setting overrides
      frontend:
        memory_allocation: 1073741824
        environment_variables:
          NODE_ENV: "development"
          
  - name: staging  
    network: testnet
    canisters: ["frontend", "backend"]
    settings:
      frontend:
        compute_allocation: 5
        environment_variables:
          NODE_ENV: "staging"
          API_URL: "https://staging-api.example.com"
      backend:
        memory_allocation: 2147483648
        
  - name: production
    network: ic
    canisters: ["frontend", "backend", "assets"]
    settings:
      frontend:
        compute_allocation: 10
        memory_allocation: 4294967296
        environment_variables:
          NODE_ENV: "production"
          API_URL: "https://api.example.com"
```

## Multi-Canister Projects

### Using Glob Patterns

```yaml
canisters:
  - canisters/*                    # All YAML files in canisters/ directory
  - services/*/canister.yaml       # Nested canister definitions
  - third-party/assets/canister.yaml  # Specific external canister
```

### Directory Structure

```
project/
├── icp.yaml                     # Main project configuration
├── canisters/
│   ├── frontend/
│   │   └── canister.yaml        # Frontend canister definition
│   ├── backend/
│   │   └── canister.yaml        # Backend canister definition
│   └── assets/
│       └── canister.yaml        # Assets canister definition
└── networks/
    ├── staging.yaml             # Staging network configuration
    └── testnet.yaml             # Testnet configuration
```

### External Canister Files

**canisters/frontend/canister.yaml:**
```yaml
name: frontend
build:
  steps:
    - type: script
      commands:
        - npm ci
        - npm run build
        - cp dist/frontend.wasm $ICP_WASM_OUTPUT_PATH
sync:
  steps:
    - type: assets
      dir: dist
```

## Complete Examples

### Single Canister with Script Build

```yaml
canister:
  name: hello-world
  build:
    steps:
      - type: script
        commands:
          - cargo build --target wasm32-unknown-unknown --release
          - cp target/wasm32-unknown-unknown/release/hello_world.wasm $ICP_WASM_OUTPUT_PATH
  settings:
    memory_allocation: 1073741824
```

### Multi-Canister with Assets

```yaml
canisters:
  - name: backend
    build:
      steps:
        - type: script
          command: cargo build --target wasm32-unknown-unknown --release --package backend
    
  - name: frontend  
    build:
      steps:
        - type: pre-built
          url: https://github.com/dfinity/sdk/raw/refs/tags/0.27.0/src/distributed/assetstorage.wasm.gz
          sha256: 865eb25df5a6d857147e078bb33c727797957247f7af2635846d65c5397b36a6
    sync:
      steps:
        - type: assets
          dirs: ["www", "assets"]

networks:
  - name: local
    mode: managed
    gateway:
      port: 8000

environments:
  - name: development
    network: local
    canisters: ["backend", "frontend"]
    settings:
      frontend:
        environment_variables:
          NODE_ENV: "development"
```

### Production Configuration with Multiple Environments

```yaml
canisters:
  - canisters/*

networks:
  - name: local
    mode: managed
  
  - name: ic  
    mode: connected
    url: https://icp0.io

environments:
  - name: local
    network: local
    canisters: ["backend", "frontend"]
    
  - name: production
    network: ic
    canisters: ["backend", "frontend", "monitoring"]
    settings:
      backend:
        compute_allocation: 10
        memory_allocation: 4294967296
        reserved_cycles_limit: 50000000000000
        environment_variables:
          NODE_ENV: "production"
          DATABASE_URL: "https://prod-db.example.com"
      frontend:
        memory_allocation: 2147483648
        environment_variables:
          NODE_ENV: "production"
          API_URL: "https://api.example.com"
```

## Validation and Best Practices

### Validation

The ICP CLI validates your configuration and provides helpful error messages:

```bash
# Validate configuration without building
icp build --dry-run

# Check specific canister configuration  
icp build my-canister --dry-run
```

### Best Practices

**Organization:**
- Use external canister files for complex multi-canister projects
- Group related canisters in subdirectories  
- Use consistent naming conventions

**Security:**
- Set appropriate freezing thresholds for production canisters
- Use reserved cycles limits to prevent runaway consumption
- Validate pre-built WASM files with SHA256 hashes
- Use environment variables instead of hardcoding sensitive values

**Performance:**
- Configure memory allocation based on actual usage patterns
- Use compute allocation for guaranteed performance in production
- Monitor WASM memory usage and set appropriate limits

**Development Workflow:**
- Use environments for different deployment stages
- Leverage recipes for consistent build processes  
- Version control your configuration alongside your code
- Use glob patterns to automatically include new canisters

## Schema Validation

This configuration format can be validated against the [JSON Schema](../icp-yaml-schema.json) provided alongside this documentation. Many editors and tools support JSON Schema validation for YAML files.

---

**Note:** This documentation reflects the current state of the codebase. The JSON Schema is automatically generated from the Rust type definitions, ensuring it's always accurate and up-to-date. If you notice discrepancies, the schema file takes precedence as it directly reflects the implementation.
