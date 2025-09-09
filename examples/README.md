# ICP CLI Examples

This directory contains a comprehensive collection of examples demonstrating various features and usage patterns of ICP CLI. Each example is a complete, working project that you can use as a starting point or reference for your own Internet Computer applications.

## Quick Start

To try any example:

1. **Copy the example** to your workspace:
   ```bash
   cp -r examples/icp-motoko my-project
   cd my-project
   ```

2. **Start local network** (in separate terminal):
   ```bash
   icp network run
   ```

3. **Deploy the canister**:
   ```bash
   icp deploy
   ```

4. **Interact with your canister**:
   ```bash
   icp canister call my-canister greet '("World")'
   ```

## Examples by Category

### 🚀 Getting Started

Perfect for new users learning ICP CLI basics.

| Example | Description | Key Features |
|---------|-------------|--------------|
| **[icp-motoko](icp-motoko/)** | Simple Motoko canister with greeting function | Script builds, basic canister interaction |
| **[icp-rust](icp-rust/)** | Simple Rust canister with greeting function | Rust compilation, WASM targeting |
| **[icp-empty](icp-empty/)** | Minimal project structure | Project skeleton, basic configuration |

**Try first:** `icp-motoko` or `icp-rust` depending on your language preference.

### 🏗️ Build Systems & Recipes

Examples demonstrating different build approaches and the recipe system.

| Example | Description | Key Features |
|---------|-------------|--------------|
| **[icp-motoko-recipe](icp-motoko-recipe/)** | Motoko canister using built-in recipe | Built-in recipe system, simplified config |
| **[icp-rust-recipe](icp-rust-recipe/)** | Rust canister using built-in recipe | Recipe-based builds, Cargo integration |
| **[icp-pre-built](icp-pre-built/)** | Deploy pre-compiled WASM | Pre-built binaries, WASM integrity checks |
| **[icp-pre-built-recipe](icp-pre-built-recipe/)** | Pre-built deployment with recipe | Recipe system for pre-built canisters |
| **[icp-recipe-local-file](icp-recipe-local-file/)** | Custom local recipe file | Local recipe development, Handlebars templating |
| **[icp-recipe-remote-url](icp-recipe-remote-url/)** | Recipe fetched from HTTP URL | Remote recipe hosting, HTTP-based recipes |
| **[icp-recipe-registry-official](icp-recipe-registry-official/)** | Official recipe registry usage | Recipe registry, versioned recipes |
| **[icp-recipe-remote-url-official](icp-recipe-remote-url-official/)** | Official remote recipe | Standardized remote recipes |

**Try first:** `icp-motoko-recipe` to understand the recipe system.

### 🌐 Static Assets & Frontend

Examples for deploying static websites and frontend applications.

| Example | Description | Key Features |
|---------|-------------|--------------|
| **[icp-static-assets](icp-static-assets/)** | Static HTML website deployment | Asset bundling, static hosting |
| **[icp-static-assets-recipe](icp-static-assets-recipe/)** | Static assets with recipe system | Asset recipes, automated deployment |
| **[icp-static-react-site](icp-static-react-site/)** | React application deployment | React builds, modern frontend tooling |

**Try first:** `icp-static-assets` for simple static sites.

### ⚙️ Configuration & Settings

Advanced configuration patterns and canister settings.

| Example | Description | Key Features |
|---------|-------------|--------------|
| **[icp-canister-settings](icp-canister-settings/)** | Comprehensive canister settings | Memory allocation, compute settings, freezing thresholds |
| **[icp-canister-environment-variables](icp-canister-environment-variables/)** | Environment variables in canisters | Runtime configuration, environment-based settings |
| **[icp-environments](icp-environments/)** | Multi-environment deployment setup | Environment management, network targeting |

**Try first:** `icp-canister-settings` to understand canister configuration.

### 🏢 Project Organization

Examples for organizing complex projects with multiple canisters.

| Example | Description | Key Features |
|---------|-------------|--------------|
| **[icp-project-single-canister](icp-project-single-canister/)** | Single canister project structure | Project organization, basic structure |
| **[icp-project-multi-canister](icp-project-multi-canister/)** | Multi-canister project with separate configs | Multi-canister coordination, project scaling |

**Try first:** `icp-project-single-canister`, then progress to multi-canister.

### 🌍 Networking

Network configuration and multi-environment deployment examples.

| Example | Description | Key Features |
|---------|-------------|--------------|
| **[icp-network-inline](icp-network-inline/)** | Inline network configuration | Network definition, custom gateways |
| **[icp-network-connected](icp-network-connected/)** | External network configuration files | Network modularity, environment separation |

**Try first:** `icp-network-inline` for simple network setup.

### 🔄 Data Synchronization

Post-deployment data synchronization and asset management.

| Example | Description | Key Features |
|---------|-------------|--------------|
| **[icp-sync](icp-sync/)** | Canister with post-deployment sync | Sync operations, post-deploy workflows |

### 📦 Dependency Management

Examples showing integration with package managers and external dependencies.

| Example | Description | Key Features |
|---------|-------------|--------------|
| **[icp-motoko-mops](icp-motoko-mops/)** | Motoko project with MOPS package manager | MOPS integration, dependency management |

### 🚀 Optimization & Performance

Advanced optimization techniques for production deployments.

| Example | Description | Key Features |
|---------|-------------|--------------|
| **[icp-wasm-metadata](icp-wasm-metadata/)** | WASM metadata injection | Build metadata, WASM optimization |
| **[icp-wasm-metadata-recipe](icp-wasm-metadata-recipe/)** | Metadata with recipe system | Recipe-based metadata handling |
| **[icp-wasm-optimization](icp-wasm-optimization/)** | WASM size and performance optimization | WASM optimization, production builds |
| **[icp-wasm-optimization-recipe](icp-wasm-optimization-recipe/)** | Optimization using recipes | Optimized recipe patterns |

### 🧪 Testing & Development

Examples for testing and development workflows.

| Example | Description | Key Features |
|---------|-------------|--------------|
| **[icp-progress-bar-test-bed](icp-progress-bar-test-bed/)** | Progress indicator testing | Development tooling, progress bars |

## Example Structure

Each example follows a consistent structure:

```
example-name/
├── README.md          # Specific instructions and explanation
├── icp.yaml          # ICP CLI configuration
├── src/              # Source code (language-specific)
├── package.json      # Node.js dependencies (if applicable)
├── Cargo.toml        # Rust dependencies (if applicable)
├── mops.toml         # MOPS dependencies (if applicable)
└── dist/             # Build output or pre-built files
```

## Learning Path

### Beginner Path
1. Start with **[icp-motoko](icp-motoko/)** or **[icp-rust](icp-rust/)**
2. Learn recipes with **[icp-motoko-recipe](icp-motoko-recipe/)**
3. Understand configuration with **[icp-canister-settings](icp-canister-settings/)**
4. Try static assets with **[icp-static-assets](icp-static-assets/)**

### Intermediate Path
1. Multi-canister projects: **[icp-project-multi-canister](icp-project-multi-canister/)**
2. Environment management: **[icp-environments](icp-environments/)**
3. Custom recipes: **[icp-recipe-local-file](icp-recipe-local-file/)**
4. Advanced configuration: **[icp-canister-environment-variables](icp-canister-environment-variables/)**

### Advanced Path
1. Network configuration: **[icp-network-connected](icp-network-connected/)**
2. WASM optimization: **[icp-wasm-optimization](icp-wasm-optimization/)**
3. Remote recipes: **[icp-recipe-remote-url](icp-recipe-remote-url/)**
4. Complex React deployments: **[icp-static-react-site](icp-static-react-site/)**

## Common Patterns

### Basic Canister Pattern
```yaml
canister:
  name: my-canister
  build:
    steps:
      - type: script
        commands:
          - # Your build commands
          - mv output.wasm "$ICP_WASM_OUTPUT_PATH"
```

### Recipe Pattern
```yaml
canister:
  name: my-canister
  recipe:
    type: rust  # or motoko, or custom URL
    configuration:
      package: my-canister
```

### Multi-Canister Pattern
```yaml
canisters:
  - canisters/*  # Glob pattern
  - path/to/specific/canister.yaml
```

### Environment Pattern
```yaml
environments:
  - name: dev
    network: local
    canisters: [frontend, backend]
    settings:
      frontend:
        memory_allocation: 1073741824
```

## Prerequisites by Example Type

### Motoko Examples
- Motoko compiler (`moc`) - Install with dfx: `dfx cache install`
- Basic Motoko knowledge

### Rust Examples  
- Rust toolchain: `rustup target add wasm32-unknown-unknown`
- Cargo and basic Rust knowledge

### Frontend Examples
- Node.js and npm/yarn
- Modern JavaScript/TypeScript knowledge

### Advanced Examples
- Understanding of Internet Computer concepts
- Knowledge of WASM and canister lifecycle

## Troubleshooting

### Common Issues

**"moc not found" error**
```bash
export PATH=$(dfx cache show):$PATH
```

**"wasm32-unknown-unknown target not found"**
```bash
rustup target add wasm32-unknown-unknown
```

**"pocket-ic not found"**
```bash
export ICP_POCKET_IC_PATH="$(dfx cache show)/pocket-ic"
```

**Build fails in example**
- Check prerequisites for that example type
- Ensure you're in the example directory
- Try `icp build --debug` for verbose output

**Network connection issues**
- Ensure `icp network run` is running in another terminal
- Check `icp network ping --wait-healthy`

### Getting Help

1. Check the example's individual README
2. Use `icp help <command>` for command-specific help
3. Try `--debug` flag for verbose output
4. Refer to the main [CLI reference](../docs/cli-reference.md)

## Contributing Examples

We welcome contributions of new examples! When adding examples:

1. Follow the established directory structure
2. Include a comprehensive README.md
3. Test the example thoroughly
4. Add it to this overview with appropriate categorization
5. Ensure it demonstrates a specific feature or pattern

See [Contributing Guidelines](../docs/contributing.md) for more details.

## Next Steps

After exploring these examples:

- Read the [Getting Started Guide](../docs/getting-started.md)
- Learn about [Project Configuration](../docs/project-configuration.md)
- Explore [Advanced Workflows](../docs/workflows.md)
- Check the [CLI Reference](../docs/cli-reference.md)

Happy building! 🚀
