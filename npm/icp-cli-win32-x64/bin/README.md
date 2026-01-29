# Binary Placeholder

This directory should contain the `icp.exe` binary for Windows x64.

## Download

Run from the repository root:

```bash
./scripts/download-binaries.sh 0.1.0-beta.4
```

Or manually download:

```bash
VERSION="0.1.0-beta.4"
curl -L "https://github.com/dfinity/icp-cli/releases/download/v${VERSION}/icp-x86_64-pc-windows-msvc.zip" -o win32-x64.zip
unzip -o win32-x64.zip -d icp-cli-win32-x64/bin/
rm win32-x64.zip
```

## Expected File

- `icp.exe` (executable binary for Windows x64)
