#!/bin/bash

set -e

# Ensure script is run from project root
if [ ! -d "icp-cli" ] || [ ! -f "icp-cli/package.json" ]; then
  echo "Error: This script must be run from the project root directory"
  echo "Usage: ./scripts/download-binaries.sh <version>"
  exit 1
fi

# Check if version argument is provided
if [ -z "$1" ]; then
  echo "Error: Version argument is required"
  echo "Usage: ./scripts/download-binaries.sh <version>"
  echo "Example: ./scripts/download-binaries.sh v0.1.0-beta.5"
  exit 1
fi

# Add leading 'v' if not present to normalize version format
VERSION="$1"
if [[ ! "$VERSION" =~ ^v ]]; then
  VERSION="v${VERSION}"
fi

echo "Downloading icp-cli binaries version $VERSION"
echo ""

# Create bin directories if they don't exist
mkdir -p icp-cli-darwin-arm64/bin
mkdir -p icp-cli-darwin-x64/bin
mkdir -p icp-cli-linux-arm64/bin
mkdir -p icp-cli-linux-x64/bin
mkdir -p icp-cli-win32-x64/bin

# Base URL for downloads
BASE_URL="https://github.com/dfinity/icp-cli/releases/download/${VERSION}"

# macOS ARM64 (Apple Silicon)
echo "Downloading macOS ARM64..."
curl -L "${BASE_URL}/icp-cli-aarch64-apple-darwin.tar.xz" -o darwin-arm64.tar.xz
mkdir -p tmp-darwin-arm64
tar -xJf darwin-arm64.tar.xz -C tmp-darwin-arm64
find tmp-darwin-arm64 -name 'icp' -type f -exec mv {} icp-cli-darwin-arm64/bin/icp \;
chmod +x icp-cli-darwin-arm64/bin/icp
rm -rf darwin-arm64.tar.xz tmp-darwin-arm64
echo "✓ macOS ARM64 downloaded"
echo ""

# macOS x64 (Intel)
echo "Downloading macOS x64..."
curl -L "${BASE_URL}/icp-cli-x86_64-apple-darwin.tar.xz" -o darwin-x64.tar.xz
mkdir -p tmp-darwin-x64
tar -xJf darwin-x64.tar.xz -C tmp-darwin-x64
find tmp-darwin-x64 -name 'icp' -type f -exec mv {} icp-cli-darwin-x64/bin/icp \;
chmod +x icp-cli-darwin-x64/bin/icp
rm -rf darwin-x64.tar.xz tmp-darwin-x64
echo "✓ macOS x64 downloaded"
echo ""

# Linux ARM64
echo "Downloading Linux ARM64..."
curl -L "${BASE_URL}/icp-cli-aarch64-unknown-linux-gnu.tar.xz" -o linux-arm64.tar.xz
mkdir -p tmp-linux-arm64
tar -xJf linux-arm64.tar.xz -C tmp-linux-arm64
find tmp-linux-arm64 -name 'icp' -type f -exec mv {} icp-cli-linux-arm64/bin/icp \;
chmod +x icp-cli-linux-arm64/bin/icp
rm -rf linux-arm64.tar.xz tmp-linux-arm64
echo "✓ Linux ARM64 downloaded"
echo ""

# Linux x64
echo "Downloading Linux x64..."
curl -L "${BASE_URL}/icp-cli-x86_64-unknown-linux-gnu.tar.xz" -o linux-x64.tar.xz
mkdir -p tmp-linux-x64
tar -xJf linux-x64.tar.xz -C tmp-linux-x64
find tmp-linux-x64 -name 'icp' -type f -exec mv {} icp-cli-linux-x64/bin/icp \;
chmod +x icp-cli-linux-x64/bin/icp
rm -rf linux-x64.tar.xz tmp-linux-x64
echo "✓ Linux x64 downloaded"
echo ""

# Windows x64
echo "Downloading Windows x64..."
curl -L "${BASE_URL}/icp-cli-x86_64-pc-windows-msvc.zip" -o win32-x64.zip
mkdir -p tmp-win32-x64
unzip -o win32-x64.zip -d tmp-win32-x64
find tmp-win32-x64 -name 'icp.exe' -type f -exec mv {} icp-cli-win32-x64/bin/icp.exe \;
rm -rf win32-x64.zip tmp-win32-x64
echo "✓ Windows x64 downloaded"

echo ""
echo "=========================================="
echo "All binaries downloaded successfully!"
echo "=========================================="
echo ""
echo "Binary locations:"
echo "  • icp-cli-darwin-arm64/bin/icp"
echo "  • icp-cli-darwin-x64/bin/icp"
echo "  • icp-cli-linux-arm64/bin/icp"
echo "  • icp-cli-linux-x64/bin/icp"
echo "  • icp-cli-win32-x64/bin/icp.exe"
echo ""
