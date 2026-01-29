# Binary Placeholder

This directory should contain the `icp` binary for macOS ARM64 (Apple Silicon).

## Download

Run from the repository root:

```bash
./scripts/download-binaries.sh 0.1.0-beta.4
```

Or manually download:

```bash
VERSION="0.1.0-beta.4"
curl -L "https://github.com/dfinity/icp-cli/releases/download/v${VERSION}/icp-aarch64-apple-darwin.tar.gz" -o darwin-arm64.tar.gz
tar -xzf darwin-arm64.tar.gz -C icp-cli-darwin-arm64/bin/
chmod +x icp-cli-darwin-arm64/bin/icp
rm darwin-arm64.tar.gz
```

## Expected File

- `icp` (executable binary for macOS ARM64)
