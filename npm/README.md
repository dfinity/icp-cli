# icp-cli npm Package

This directory contains the npm package distribution system for `icp-cli` using the **bundled binaries per-architecture** approach.

## Directory Structure

- **`icp-cli/`** - Main wrapper package with binary wrapper script and programmatic API
- **`icp-cli-{platform}-{arch}/`** - Platform-specific packages containing pre-compiled binaries
- **`scripts/`** - Build and deployment automation scripts
- **`Dockerfile.test`** - Docker-based testing environment
- **`docker-compose.test.yml`** - Multi-version Node.js testing

## Automated Release Process

The npm packages are automatically published when a GitHub release is created:

1. Developer pushes a version tag (e.g., `v0.1.0-beta.6`) to the main icp-cli repository
2. The `release.yml` workflow (managed by cargo-dist) builds and releases Rust binaries
3. When the GitHub release is published, the `release-npm.yml` workflow automatically:
   - Downloads the newly released binaries from the GitHub release
   - Updates package.json versions to match
   - Runs Docker tests
   - Publishes all 6 packages to npm

See [`.github/workflows/release-npm.yml`](../.github/workflows/release-npm.yml) for the complete workflow.

## Manual Testing (for Development)

If you need to test the npm packages locally before a release:

### 1. Download Binaries

From the `npm` directory, download the pre-compiled binaries:

```bash
cd npm
./scripts/download-binaries.sh v0.1.0-beta.3
```

Or manually download from [icp-cli releases](https://github.com/dfinity/icp-cli/releases) and place them in the respective `bin/` directories.

### 2. Verify Binaries

```bash
./scripts/verify-binaries.sh
```

### 3. Test Locally

```bash
./scripts/test-docker.sh quick  # Quick test on Node 20
./scripts/test-docker.sh full   # Full test on Node 18, 20, 22
```

### 4. Update Version (if testing a specific version)

```bash
./scripts/update-package-json.sh 0.1.0-beta.3
```

This will update the version in all packages and their dependencies.

## Usage After Publishing

Users can install the package globally:

```bash
npm install -g icp-cli
```

Or locally in their project:

```bash
npm install icp-cli
```

Then use it:

```bash
icp --help
```

Or programmatically:

```javascript
const icp = require('icp-cli');
console.log('Binary location:', icp.binaryPath);
```

## Scripts Reference

All scripts are located in the `scripts/` directory and should be run from the `npm/` directory:

- **`download-binaries.sh <version>`** - Downloads binaries from GitHub releases for all platforms
- **`verify-binaries.sh`** - Verifies all binaries are present and have correct permissions
- **`update-package-json.sh <version>`** - Updates version in all package.json files
- **`publish-all.sh <version>`** - Publishes all packages to npm
- **`test-docker.sh [quick|full|interactive|clean]`** - Docker-based testing

## Architecture

### Package Structure

- **Main package**: `icp-cli` - Contains the wrapper script and API, depends on platform packages
- **Platform packages**: 5 packages for different OS/architecture combinations
  - `icp-cli-darwin-arm64` (macOS Apple Silicon)
  - `icp-cli-darwin-x64` (macOS Intel)
  - `icp-cli-linux-arm64` (Linux ARM64)
  - `icp-cli-linux-x64` (Linux x64)
  - `icp-cli-win32-x64` (Windows x64)

### Binary Distribution

Binaries are:
- Downloaded from GitHub releases during the CI/CD process
- Not stored in git (ignored via `.gitignore`)
- Packaged per platform using npm's `os` and `cpu` restrictions
- Automatically selected at install time via `optionalDependencies`

## Maintenance

### Releasing a New Version

Releases are fully automated:

1. Update version in `icp-cli/Cargo.toml`
2. Commit changes and push a version tag:
   ```bash
   git tag v0.1.0-beta.6
   git push origin v0.1.0-beta.6
   ```
3. The `release.yml` workflow (cargo-dist) will:
   - Build Rust binaries for all platforms
   - Create and publish a GitHub release
4. The `release-npm.yml` workflow will then:
   - Detect the new release
   - Download binaries and publish npm packages

No manual npm publishing is required! The two workflows run independently to avoid conflicts with cargo-dist's workflow management.
