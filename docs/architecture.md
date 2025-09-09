# ICP CLI Architecture

This document provides a comprehensive overview of the ICP CLI architecture, including its core components, data flow, and design principles.

## Overview

ICP CLI is designed as a modular, extensible system built in Rust. It follows a layered architecture with clear separation of concerns between configuration parsing, build execution, network management, and deployment operations.

```
┌─────────────────────────────────────────────────────────────┐
│                   CLI Interface                             │
│                   (bin/icp-cli)                             │
├─────────────────────────────────────────────────────────────┤
│                   Command Layer                             │
│                (Command Dispatch)                           │
├─────────────────────────────────────────────────────────────┤
│     Project      │    Network     │   Identity   │  Build   │
│   Management     │   Management   │  Management  │ Adapters │
│  (icp-project)   │ (icp-network)  │(icp-identity)│(adapter) │
├─────────────────────────────────────────────────────────────┤
│             Configuration & File System Layer               │
│              (icp-fs, icp-dirs, icp-canister)               │
├─────────────────────────────────────────────────────────────┤
│                   External Dependencies                     │
│              (IC Agent, Pocket IC, dfx tools)               │
└─────────────────────────────────────────────────────────────┘
```

## Core Components

### CLI Binary (`bin/icp-cli/`)

The main executable that provides the command-line interface.

**Key responsibilities:**
- Command-line parsing with `clap`
- Context initialization and global configuration
- Command dispatch and error handling
- Progress indication and user feedback
- Telemetry and analytics collection

**Main modules:**
- `main.rs` - Entry point and CLI setup
- `commands/` - Command implementations
- `context.rs` - Global application context
- `store_artifact.rs` - Artifact storage management
- `store_id.rs` - Canister ID persistence

### Project Management (`lib/icp-project/`)

Handles project-level configuration and canister orchestration.

**Key responsibilities:**
- `icp.yaml` parsing and validation
- Project structure discovery
- Multi-canister coordination
- Environment and network resolution

**Core types:**
- `Project` - Top-level project representation
- `ProjectManifest` - Parsed `icp.yaml` structure
- `Canister` - Individual canister configuration
- `EnvironmentManifest` - Environment-specific settings

### Network Management (`lib/icp-network/`)

Manages Internet Computer network connections and configurations.

**Key responsibilities:**
- Network discovery and configuration
- Local network management (pocket-ic integration)
- Gateway connection handling
- Network health monitoring

**Architecture:**
- `NetworkConfig` - Network connection parameters
- `ManagedNetwork` - Local development networks
- `NetworkStatus` - Health and connectivity monitoring

### Identity Management (`lib/icp-identity/`)

Handles cryptographic identities and key management.

**Key responsibilities:**
- Identity creation and import
- Key derivation and storage
- Principal computation
- Authentication with IC networks

**Components:**
- `Identity` - Core identity representation
- `IdentityManager` - Identity lifecycle management
- Key storage and encryption

### Build Adapters (`lib/icp-adapter/`)

Provides pluggable build system integration.

**Adapter Types:**
- `ScriptAdapter` - Custom shell command execution
- `PrebuiltAdapter` - Pre-compiled WASM handling
- `AssetsAdapter` - Static asset bundling

**Architecture:**
```rust
trait BuildAdapter {
    async fn build(&self, context: &BuildContext) -> Result<PathBuf>;
}
```

### Canister Management (`lib/icp-canister/`)

Handles canister-specific configuration and operations.

**Key responsibilities:**
- Canister manifest parsing
- Recipe system implementation
- Build step orchestration
- Sync operation management

**Core concepts:**
- `BuildSteps` - Sequential build operations
- `SyncSteps` - Post-deployment synchronization
- `Recipe` - Reusable build templates
- `CanisterSettings` - Runtime configuration

### File System Utilities (`lib/icp-fs/`)

Provides safe file system operations and configuration management.

**Features:**
- Atomic file operations with locking
- YAML/JSON configuration handling
- Temporary file management
- Cross-platform path handling

### Directory Management (`lib/icp-dirs/`)

Manages application directories and file organization.

**Responsibilities:**
- User configuration directories
- Project-specific directories
- Cache and temporary file locations
- Cross-platform directory standards

## Data Flow

### Project Initialization

```
User Command → CLI Parser → Context Setup → Project Discovery
                                      ↓
Directory Scan ← Configuration Load ← Manifest Parse ← File System
                                      ↓
     ↓                          Validation & Resolution
Network Config ← Environment Setup ← Identity Setup ← Project Setup
```

### Build Process

```
Build Command → Project Load → Canister Discovery → Build Planning
                                      ↓
Recipe Resolution → Build Step Generation → Adapter Selection
                                      ↓
Build Execution → Artifact Generation → Validation → Storage
```

### Deployment Process

```
Deploy Command → Network Connection → Identity Authentication
                                      ↓
Canister Creation ← Build Artifacts ← Build Process ← Project Config
                                      ↓
Code Installation → Settings Application → Post-Deploy Sync
```

## Design Principles

### Modularity

Each library crate has a single, well-defined responsibility:
- Clear interfaces between components
- Minimal coupling between modules
- Pluggable architecture for extensibility

### Configuration-Driven

All behavior is driven by declarative configuration:
- `icp.yaml` as single source of truth
- Environment-specific overrides
- Recipe system for reusable configurations

### Async-First

Built for concurrent operations:
- Tokio async runtime throughout
- Concurrent canister builds
- Non-blocking network operations
- Stream-based processing for large datasets

### Error Handling

Comprehensive error handling with context:
- `snafu` for structured error types
- Rich error context and suggestions
- Graceful failure modes
- User-friendly error messages

### Extensibility

Designed for future growth:
- Plugin architecture via adapters
- Recipe system for custom workflows
- Hook points for external tools
- API-friendly internal design

## Adapter System

The adapter system provides pluggable build and sync operations:

### Build Adapters

```rust
#[async_trait]
pub trait BuildAdapter {
    async fn build(&self, context: &BuildContext) -> Result<Utf8PathBuf, BuildError>;
}
```

**Current implementations:**
- `ScriptAdapter` - Execute shell commands
- `PrebuiltAdapter` - Use existing WASM files
- Future: Rust, Motoko, JavaScript adapters

### Sync Adapters

```rust  
#[async_trait]
pub trait SyncAdapter {
    async fn sync(&self, context: &SyncContext) -> Result<(), SyncError>;
}
```

**Current implementations:**
- `ScriptAdapter` - Custom sync commands
- `AssetsAdapter` - Static asset uploads

## Recipe System

Recipes provide templated, reusable build configurations:

### Local Recipes
```yaml
canister:
  recipe:
    type: file://./recipes/rust-optimized.hb.yaml
    configuration:
      package: my-canister
      profile: release
```

### Remote Recipes
```yaml
canister:
  recipe:  
    type: https://recipes.ic-cli.org/rust/v1.yaml
    configuration:
      package: my-canister
```

### Built-in Recipes
```yaml
canister:
  recipe:
    type: rust  # Built-in recipe identifier
    configuration:
      package: my-canister
```

**Recipe Resolution Process:**
1. Parse recipe type and configuration
2. Fetch recipe template (local file, URL, or built-in)
3. Render template with Handlebars
4. Parse resulting build/sync steps
5. Execute generated steps

## State Management

### Persistent State

- **ID Store** (`.icp/ids.json`) - Canister ID mappings
- **Artifact Store** (`.icp/artifacts/`) - Build artifacts and metadata
- **Identity Store** - Cryptographic identities and keys

### Runtime State

- **Project Context** - Current project configuration
- **Network Context** - Active network connections
- **Build Context** - Build-specific state and metadata

### Concurrency Model

- **Tokio Runtime** - Async task execution
- **File Locking** - Prevents concurrent modification
- **Channel Communication** - Inter-task messaging
- **Shared State** - Arc-wrapped thread-safe containers

## Integration Points

### External Tools

**pocket-ic**: Local IC replica for development
```rust
// Network management integrates with pocket-ic
let network = ManagedNetwork::start("pocket-ic", config).await?;
```

**dfx**: Compatibility and tool integration
```rust
// Use dfx-provided tools when available
let moc_path = find_dfx_tool("moc").unwrap_or("moc".into());
```

**ic-agent**: Internet Computer protocol client
```rust
// Network layer uses ic-agent for IC communication
let agent = Agent::builder()
    .with_url(network.gateway_url())
    .build()?;
```

### File System Integration

```rust
// Safe file operations with locking
let locked_json = LockedJson::<IdStore>::open(id_store_path)?;
locked_json.update(|store| store.set_id("canister", principal))?;
```

## Performance Considerations

### Parallel Builds
- Concurrent canister building
- Recipe resolution parallelization
- Independent build step execution

### Caching
- Artifact caching between builds
- Recipe template caching
- Network response caching

### Resource Management
- Bounded task pools for builds
- Memory-efficient streaming for large files
- Proper resource cleanup on errors

## Security Model

### Identity Management
- Secure key storage with encryption
- BIP39 seed phrase support
- Hardware wallet integration (future)

### Network Security
- TLS for all network communications
- Certificate validation
- Secure credential storage

### Build Security
- WASM integrity verification (SHA256)
- Isolated build environments
- Input validation and sanitization

## Future Architecture Enhancements

### Plugin System
- Dynamic plugin loading
- Third-party adapter registration
- Plugin marketplace integration

### Distributed Builds
- Remote build execution
- Build caching and distribution
- Horizontal scaling support

### Advanced Tooling
- Language server protocol support
- IDE integration APIs
- Debug adapter protocol implementation

## Debugging and Observability

### Logging
- Structured logging with `tracing`
- Configurable log levels
- Context-aware log messages

### Telemetry
- Anonymous usage analytics
- Performance metrics collection
- Error reporting and aggregation

### Debugging Tools
- `--debug` flag for verbose output
- Build artifact inspection
- Network connection diagnostics
- Configuration validation tools

This architecture enables ICP CLI to be maintainable, extensible, and performant while providing a great developer experience for Internet Computer application development.
