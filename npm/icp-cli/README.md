# @icp-sdk/icp-cli

npm package for [icp-cli](https://github.com/dfinity/icp-cli) with pre-compiled binaries.

## Installation

```bash
npm install -g @icp-sdk/icp-cli
```

### Linux Users

On Linux, you may need to install system dependencies first:

```bash
# Ubuntu/Debian
sudo apt-get update
sudo apt-get install -y libdbus-1-3 libssl3 ca-certificates

# Fedora/RHEL
sudo dnf install -y dbus-libs openssl ca-certificates
```

Then install icp-cli:

```bash
npm install -g @icp-sdk/icp-cli
```

## Usage

```bash
icp --help
icp --version
```

## How it Works

This package uses platform-specific optional dependencies to install the correct pre-compiled binary for your system. The binary is ready to use immediately after installation - no additional downloads required.

### Supported Platforms

- macOS ARM64 (Apple Silicon)
- macOS x64 (Intel)
- Linux ARM64
- Linux x64
- Windows x64

### Programmatic Usage

```javascript
const icp = require('@icp-sdk/icp-cli');

console.log('icp binary location:', icp.binaryPath);
console.log('icp version:', icp.version);
```
