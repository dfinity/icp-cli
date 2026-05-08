#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

VERSION_FILE="$WORKSPACE_ROOT/crates/icp-cli/test-network-launcher-version"
TARGET_DIR="$WORKSPACE_ROOT/target/test-fixture"
VERSION_CACHE="$TARGET_DIR/network-launcher-version"

VERSION=$(tr -d '[:space:]' < "$VERSION_FILE" | sed 's/^v//')
PKG_VERSION="v$VERSION"

cached_version=$(tr -d '[:space:]' < "$VERSION_CACHE" 2>/dev/null || true)
if [ "$cached_version" = "$VERSION" ] \
    && [ -f "$TARGET_DIR/icp-cli-network-launcher" ] \
    && [ -f "$TARGET_DIR/pocket-ic" ]; then
    echo "Network launcher $PKG_VERSION already present, skipping download."
    exit 0
fi

ARCH=$(uname -m)
case "$ARCH" in
    aarch64|arm64) ARCH="arm64" ;;
    x86_64) ARCH="x86_64" ;;
    *) echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

OS=$(uname -s | tr '[:upper:]' '[:lower:]')
case "$OS" in
    darwin) OS="darwin" ;;
    linux) OS="linux" ;;
    *) echo "Unsupported OS: $OS" >&2; exit 1 ;;
esac

TARBALL_NAME="icp-cli-network-launcher-${ARCH}-${OS}-${PKG_VERSION}"
URL="https://github.com/dfinity/icp-cli-network-launcher/releases/download/${PKG_VERSION}/${TARBALL_NAME}.tar.gz"

echo "Downloading network launcher $PKG_VERSION from: $URL"

mkdir -p "$TARGET_DIR"
TMP_TARBALL=$(mktemp /tmp/icp-cli-network-launcher-XXXX.tar.gz)
trap 'rm -f "$TMP_TARBALL"' EXIT

curl -fSL "$URL" -o "$TMP_TARBALL"
tar -xzf "$TMP_TARBALL" --strip-components=1 -C "$TARGET_DIR"
printf '%s' "$VERSION" > "$VERSION_CACHE"

echo "Network launcher $PKG_VERSION downloaded to $TARGET_DIR"
