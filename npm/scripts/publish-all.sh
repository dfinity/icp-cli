#!/bin/bash

set -e

# Ensure script is run from project root
if [ ! -d "icp-cli" ] || [ ! -f "icp-cli/package.json" ]; then
  echo "Error: This script must be run from the project root directory"
  echo "Usage: ./scripts/publish-all.sh <version>"
  exit 1
fi

# Check if version argument is provided
if [ -z "$1" ]; then
  echo "Error: Version argument is required"
  echo "Usage: ./scripts/publish-all.sh <version>"
  echo "Example: ./scripts/publish-all.sh 0.1.0-beta.5"
  exit 1
fi

VERSION="$1"

echo "Publishing version $VERSION"

# Array of platform packages
PLATFORMS=(
  "icp-cli-darwin-arm64"
  "icp-cli-darwin-x64"
  "icp-cli-linux-arm64"
  "icp-cli-linux-x64"
  "icp-cli-win32-x64"
)

# Function to check package version
check_version() {
  local package_dir="$1"
  local package_json="$package_dir/package.json"
  
  if [ ! -f "$package_json" ]; then
    echo "Error: $package_json not found"
    exit 1
  fi
  
  local pkg_version=$(node -p "require('./$package_json').version")
  
  if [ "$pkg_version" != "$VERSION" ]; then
    echo "Error: Version mismatch in $package_dir"
    echo "  Expected: $VERSION"
    echo "  Found: $pkg_version"
    exit 1
  fi
  
  echo "✓ $package_dir version matches: $VERSION"
}

# Verify versions before publishing
echo "Verifying package versions..."
for platform in "${PLATFORMS[@]}"; do
  check_version "$platform"
done
check_version "icp-cli"
echo "All versions verified!"
echo ""

# Determine if this is a pre-release version
BETA_TAG=""
if [[ "$VERSION" =~ (alpha|beta) ]]; then
  BETA_TAG="--tag beta"
  echo "Pre-release version detected, will publish with --tag beta"
else
  echo "Stable version detected, will publish as latest"
fi
echo ""

# Publish platform packages
for platform in "${PLATFORMS[@]}"; do
  echo "Publishing $platform..."
  cd "$platform"
  npm publish --access public $BETA_TAG --provenance
  cd ..
  echo "✓ $platform published"
done

# Publish main package
echo "Publishing main package icp-cli..."
cd icp-cli
npm publish --access public $BETA_TAG --provenance
cd ..
echo "✓ icp-cli published"

echo ""
echo "All packages published successfully!"
