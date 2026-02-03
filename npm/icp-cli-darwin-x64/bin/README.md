# Binary Placeholder

This directory should contain the `icp` binary for macOS x64 (Intel).

## Download

Run from the npm directory:

```bash
cd npm
./scripts/download-binaries.sh 0.1.0
```

Or manually download:

```bash
VERSION="0.1.0"
curl -L "https://github.com/dfinity/icp-cli/releases/download/v${VERSION}/icp-x86_64-apple-darwin.tar.gz" -o darwin-x64.tar.gz
tar -xzf darwin-x64.tar.gz -C icp-cli-darwin-x64/bin/
chmod +x icp-cli-darwin-x64/bin/icp
rm darwin-x64.tar.gz
```

## Expected File

- `icp` (executable binary for macOS x64)
