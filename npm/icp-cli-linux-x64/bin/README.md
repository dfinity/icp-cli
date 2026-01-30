# Binary Placeholder

This directory should contain the `icp` binary for Linux x64.

## Download

Run from the npm directory:

```bash
cd npm
./scripts/download-binaries.sh 0.1.0-beta.4
```

Or manually download:

```bash
VERSION="0.1.0-beta.4"
curl -L "https://github.com/dfinity/icp-cli/releases/download/v${VERSION}/icp-x86_64-unknown-linux-gnu.tar.gz" -o linux-x64.tar.gz
tar -xzf linux-x64.tar.gz -C icp-cli-linux-x64/bin/
chmod +x icp-cli-linux-x64/bin/icp
rm linux-x64.tar.gz
```

## Expected File

- `icp` (executable binary for Linux x64)
