# Contributing to ICP CLI

Thank you for your interest in contributing to ICP CLI! This guide provides comprehensive information about contributing to the project, from setting up your development environment to submitting pull requests.

## Table of Contents

- [Getting Started](#getting-started)
- [Development Environment](#development-environment)
- [Project Structure](#project-structure)
- [Code Organization](#code-organization)
- [Development Workflow](#development-workflow)
- [Testing](#testing)
- [Code Style](#code-style)
- [Submitting Changes](#submitting-changes)
- [Documentation](#documentation)
- [Community Guidelines](#community-guidelines)

## Getting Started

### Prerequisites

Before contributing, ensure you have:

- **Rust toolchain** (stable): Install via [rustup](https://rustup.rs/)
- **dfx**: Install the [DFINITY SDK](https://internetcomputer.org/docs/building-apps/getting-started/install)
- **Git**: For version control and contribution workflow
- **Editor with Rust support**: VS Code with rust-analyzer recommended

### Fork and Clone

1. Fork the repository on GitHub
2. Clone your fork:
   ```bash
   git clone https://github.com/your-username/icp-cli.git
   cd icp-cli
   ```
3. Add upstream remote:
   ```bash
   git remote add upstream https://github.com/dfinity/icp-cli.git
   ```

## Development Environment

### Initial Setup

```bash
# Install Rust dependencies and build
cargo build

# Install development dependencies
cargo build --all-targets

# Set up testing environment  
dfx cache install
export ICPTEST_DFX_PATH="$(dfx cache show)/dfx"
export ICP_POCKET_IC_PATH="$(dfx cache show)/pocket-ic"

# Add built CLI to PATH for testing
export PATH=$(pwd)/target/debug:$PATH
```

### IDE Configuration

#### VS Code (Recommended)

Install these extensions:
- `rust-lang.rust-analyzer` - Rust language support
- `tamasfe.even-better-toml` - TOML file support
- `ms-vscode.test-adapter-converter` - Test integration

**Recommended settings** (`.vscode/settings.json`):
```json
{
  "rust-analyzer.cargo.loadOutDirsFromCheck": true,
  "rust-analyzer.procMacro.enable": true,
  "rust-analyzer.cargo.features": "all",
  "files.exclude": {
    "**/target": true
  }
}
```

#### Other IDEs

- **IntelliJ/CLion**: Install the Rust plugin
- **Vim/Neovim**: Use rust.vim + Coc.nvim or native LSP
- **Emacs**: Use rust-mode + lsp-rust-analyzer

## Project Structure

ICP CLI follows a workspace-based architecture:

```
icp-cli/
├── bin/
│   └── icp-cli/           # Main CLI binary
├── lib/                   # Library crates
│   ├── icp-adapter/       # Build adapters (script, pre-built, assets)
│   ├── icp-canister/      # Canister management and recipes
│   ├── icp-dirs/          # Directory utilities
│   ├── icp-fs/            # File system operations
│   ├── icp-identity/      # Identity management
│   ├── icp-network/       # Network configuration and management
│   └── icp-project/       # Project configuration and structure
├── examples/              # Example projects and templates
├── docs/                  # Documentation
├── scripts/               # Build and maintenance scripts
├── Cargo.toml            # Workspace configuration
└── Cargo.lock            # Dependency lock file
```

### Crate Responsibilities

| Crate | Purpose | Key Components |
|-------|---------|----------------|
| `icp-cli` | Main CLI interface | Command parsing, dispatch, context |
| `icp-project` | Project management | Project discovery, configuration parsing |
| `icp-canister` | Canister operations | Build steps, recipes, settings |
| `icp-adapter` | Build adapters | Script, pre-built, asset adapters |
| `icp-network` | Network management | Local networks, IC connections |
| `icp-identity` | Identity operations | Key management, authentication |
| `icp-fs` | File operations | Safe I/O, configuration files |
| `icp-dirs` | Directory utilities | Path management, standards |

## Code Organization

### Module Structure

Each library crate follows this pattern:

```rust
// lib.rs - Public API and re-exports
pub mod config;     // Configuration types
pub mod error;      // Error definitions
pub mod operations; // Core operations
mod internal;       // Private implementation details

pub use config::*;
pub use error::*;
```

### Error Handling

Use `snafu` for structured error handling:

```rust
use snafu::{Snafu, ResultExt, ensure};

#[derive(Debug, Snafu)]
pub enum MyError {
    #[snafu(display("Failed to read file {}: {}", path.display(), source))]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    
    #[snafu(display("Invalid configuration: {}", message))]
    InvalidConfig { message: String },
}

type Result<T, E = MyError> = std::result::Result<T, E>;
```

### Async Code

Use `tokio` for async operations:

```rust
use tokio::fs;
use futures::stream::{self, StreamExt};

pub async fn process_canisters(canisters: Vec<Canister>) -> Result<Vec<ProcessedCanister>> {
    // Process canisters concurrently with controlled parallelism
    let results = stream::iter(canisters)
        .map(|canister| async move { process_canister(canister).await })
        .buffer_unordered(4) // Max 4 concurrent operations
        .collect::<Vec<_>>()
        .await;
    
    results.into_iter().collect()
}
```

## Development Workflow

### Feature Development

1. **Create a feature branch**:
   ```bash
   git checkout -b feature/my-new-feature
   ```

2. **Make incremental commits**:
   ```bash
   git add .
   git commit -m "feat: add support for custom adapters"
   ```

3. **Keep branch updated**:
   ```bash
   git fetch upstream
   git rebase upstream/main
   ```

4. **Test thoroughly**:
   ```bash
   cargo test
   cargo clippy
   cargo fmt --check
   ```

### Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` - New features
- `fix:` - Bug fixes
- `docs:` - Documentation changes
- `style:` - Code style changes
- `refactor:` - Code refactoring
- `test:` - Test additions or changes
- `chore:` - Maintenance tasks

**Examples:**
```
feat: add support for remote recipe URLs
fix: resolve canister ID lookup in multi-project workspaces
docs: add configuration examples for multi-environment setups
```

### Branch Naming

Use descriptive branch names:
- `feature/recipe-system` - New features
- `fix/canister-deployment-bug` - Bug fixes  
- `docs/api-reference-update` - Documentation
- `refactor/network-layer` - Code refactoring

## Testing

### Test Organization

```
src/
├── lib.rs
├── config.rs
├── operations.rs
└── tests/           # Integration tests
    ├── mod.rs
    ├── config_test.rs
    └── operations_test.rs

tests/               # End-to-end tests
├── cli_integration.rs
└── fixtures/
    └── test_projects/
```

### Unit Tests

Write unit tests alongside your code:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_parse_configuration() {
        let config_yaml = r#"
            canister:
              name: test-canister
              build:
                steps:
                  - type: script
                    commands: ["echo 'building'"]
        "#;
        
        let config: CanisterManifest = serde_yaml::from_str(config_yaml).unwrap();
        assert_eq!(config.name, "test-canister");
    }
    
    #[tokio::test]
    async fn test_async_operation() {
        let result = async_operation().await;
        assert!(result.is_ok());
    }
}
```

### Integration Tests

Test CLI commands end-to-end:

```rust
// tests/cli_integration.rs
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn test_build_command() {
    let temp_dir = TempDir::new().unwrap();
    
    // Set up test project
    setup_test_project(&temp_dir);
    
    let mut cmd = Command::cargo_bin("icp").unwrap();
    cmd.current_dir(&temp_dir)
        .arg("build")
        .arg("test-canister");
        
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Build completed"));
}
```

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_parse_configuration

# Run integration tests only
cargo test --test cli_integration

# Run tests with environment setup
ICPTEST_DFX_PATH="$(dfx cache show)/dfx" \
ICP_POCKET_IC_PATH="$(dfx cache show)/pocket-ic" \
cargo test
```

### Test Isolation

Use `serial_test` for tests that need isolation:

```rust
use serial_test::serial;

#[test]
#[serial]
fn test_network_operations() {
    // Tests that modify global network state
}
```

## Code Style

### Formatting

Use `rustfmt` with project configuration:

```bash
# Format all code
cargo fmt

# Check formatting
cargo fmt -- --check
```

### Linting

Use `clippy` for additional linting:

```bash
# Run clippy
cargo clippy

# Run clippy on all targets
cargo clippy --all-targets

# Treat warnings as errors (CI)
cargo clippy -- -D warnings
```

### Code Conventions

- **Naming**: Use `snake_case` for functions/variables, `PascalCase` for types
- **Visibility**: Minimize public APIs, use `pub(crate)` when appropriate
- **Documentation**: Document all public APIs with `///` comments
- **Dependencies**: Keep external dependencies minimal and well-justified

**Example:**
```rust
/// Represents a canister build configuration.
/// 
/// # Examples
/// 
/// ```rust
/// use icp_canister::BuildConfig;
/// 
/// let config = BuildConfig::new("my-canister")
///     .with_script_step("cargo build --release");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct BuildConfig {
    name: String,
    steps: Vec<BuildStep>,
}

impl BuildConfig {
    /// Creates a new build configuration with the specified name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            steps: Vec::new(),
        }
    }
    
    /// Adds a script build step.
    pub fn with_script_step(mut self, command: impl Into<String>) -> Self {
        self.steps.push(BuildStep::Script(command.into()));
        self
    }
}
```

## Submitting Changes

### Pull Request Process

1. **Ensure tests pass**:
   ```bash
   cargo test
   cargo clippy
   cargo fmt --check
   ```

2. **Update documentation** if needed

3. **Create pull request** with:
   - Clear title and description
   - Reference to related issues
   - Summary of changes
   - Breaking changes (if any)

### PR Template

```markdown
## Description
Brief description of changes made.

## Related Issues
Fixes #123

## Type of Change
- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality) 
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Documentation update

## Testing
- [ ] Unit tests added/updated
- [ ] Integration tests added/updated
- [ ] Manual testing performed

## Checklist
- [ ] Code follows project style guidelines
- [ ] Self-review of code completed
- [ ] Documentation updated
- [ ] Tests added that prove fix/feature works
- [ ] All tests pass locally
```

### Review Process

1. **Automated checks** must pass
2. **Peer review** required from maintainers
3. **Testing** on multiple platforms (if applicable)
4. **Documentation** review for user-facing changes

## Documentation

### Code Documentation

Document public APIs thoroughly:

```rust
/// Builds a canister using the specified configuration.
/// 
/// # Arguments
/// 
/// * `config` - The build configuration containing steps and settings
/// * `context` - Build context with environment and paths
/// 
/// # Returns
/// 
/// Returns the path to the built WASM file on success.
/// 
/// # Errors
/// 
/// Returns `BuildError` if:
/// - Build steps fail to execute
/// - Output WASM file is not generated
/// - File system operations fail
/// 
/// # Examples
/// 
/// ```rust
/// use icp_adapter::build_canister;
/// 
/// let config = BuildConfig::new("my-canister");
/// let wasm_path = build_canister(&config, &context).await?;
/// ```
pub async fn build_canister(
    config: &BuildConfig,
    context: &BuildContext,
) -> Result<PathBuf, BuildError> {
    // Implementation
}
```

### User Documentation

Update relevant documentation files:

- **User guides**: `docs/getting-started.md`, `docs/workflows.md`
- **API reference**: Auto-generated from code comments
- **Examples**: Add new examples or update existing ones
- **CLI reference**: Auto-generated from clap definitions

### Documentation Commands

```bash
# Generate CLI documentation
./scripts/generate-cli-docs.sh

# Build API documentation  
cargo doc --open

# Check documentation builds
cargo doc --no-deps
```

## Community Guidelines

### Code of Conduct

- Be respectful and inclusive
- Focus on constructive feedback
- Help newcomers get started
- Report issues through appropriate channels

### Communication

- **GitHub Issues**: Bug reports, feature requests
- **GitHub Discussions**: Questions, ideas, general discussion  
- **Pull Requests**: Code contributions with discussion

### Getting Help

If you need help:

1. Check existing documentation
2. Search GitHub issues
3. Ask in GitHub Discussions
4. Reach out to maintainers

## Development Tips

### Performance

- Use `cargo build --release` for performance testing
- Profile with `cargo flamegraph` for bottlenecks
- Consider memory usage with large projects
- Use `Arc` and `Rc` appropriately for shared data

### Debugging

```bash
# Debug builds retain symbols
cargo build

# Run with debug logs
RUST_LOG=debug ./target/debug/icp build

# Use rust-gdb or rust-lldb for debugging
rust-gdb ./target/debug/icp
```

### Useful Commands

```bash
# Check for unused dependencies
cargo machete

# Security audit
cargo audit

# Check for updates
cargo outdated

# Expand macros
cargo expand

# Check minimal versions
cargo minimal-versions check
```

## Release Process

Maintainers handle releases, but contributors should:

- Mark breaking changes clearly
- Update `CHANGELOG.md` for significant changes
- Ensure semver compatibility

Thank you for contributing to ICP CLI! Your contributions help make Internet Computer development more accessible and enjoyable for everyone.
