# icp-cli npm Package

This directory contains the npm package distribution system for `icp-cli` using the **bundled binaries per-architecture** approach.

## Directory Structure

- **`icp-cli/`** - Main wrapper package with binary wrapper script and programmatic API
- **`icp-cli-{platform}-{arch}/`** - Platform-specific packages containing pre-compiled binaries
- **`scripts/`** - Build and deployment automation scripts
- **`Dockerfile.test`** - Docker-based testing environment
- **`docker-compose.test.yml`** - Multi-version Node.js testing

## Automated Release Process


1. Go to the repository's Actions tab
2. Select the "Publish to npm" workflow
3. Click "Run workflow" and provide:
   - **version**: Release version tag to download binaries from (e.g., `v0.1.0-beta.6`)
   - **npm_package_version** (optional): NPM package version if it should differ from the release version (e.g., `0.1.0-beta.7`)
   - **beta**: Whether to publish as a beta release (tags packages with `beta` on npm)

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
./scripts/test-docker.sh full   # Full test on Node 18, 20, 22, 24
```

### 4. Update Version (if testing a specific version)

```bash
./scripts/update-package-json.sh 0.1.0-beta.3
```

This will update the version in all packages and their dependencies.

## Usage After Publishing

Users can install the package globally:

```bash
npm install -g @icp-sdk/icp-cli
```

Or locally in their project:

```bash
npm install @icp-sdk/icp-cli
```

Then use it:

```bash
icp --help
```

Or programmatically:

```javascript
const icp = require('@icp-sdk/icp-cli');
console.log('Binary location:', icp.binaryPath);
```

## Scripts Reference

All scripts are located in the `scripts/` directory and should be run from the `npm/` directory:

- **`download-binaries.sh <version>`** - Downloads binaries from GitHub releases for all platforms
- **`verify-binaries.sh`** - Verifies all binaries are present and have correct permissions
- **`update-package-json.sh <version>`** - Updates version in all package.json files
- **`publish-all.sh <version> [tag]`** - Publishes all packages to npm (optionally with a custom tag like `beta`)
- **`test-docker.sh [quick|full]`** - Docker-based testing

## Architecture

### Package Structure

- **Main package**: `@icp-sdk/icp-cli` - Contains the wrapper script and API, depends on platform packages
- **Platform packages**: 5 packages for different OS/architecture combinations
  - `@icp-sdk/icp-cli-darwin-arm64` (macOS Apple Silicon)
  - `@icp-sdk/icp-cli-darwin-x64` (macOS Intel)
  - `@icp-sdk/icp-cli-linux-arm64` (Linux ARM64)
  - `@icp-sdk/icp-cli-linux-x64` (Linux x64)
  - `@icp-sdk/icp-cli-win32-x64` (Windows x64)

### Binary Distribution

Binaries are:
- Downloaded from GitHub releases during the CI/CD process
- Not stored in git (ignored via `.gitignore`)
- Packaged per platform using npm's `os` and `cpu` restrictions
- Automatically selected at install time via `optionalDependencies`
