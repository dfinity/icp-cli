#!/bin/bash

set -e

# Ensure script is run from npm directory
if [ ! -d "icp-cli" ] || [ ! -f "icp-cli/package.json" ]; then
  echo "Error: This script must be run from the npm directory"
  echo "Usage: cd npm && ./scripts/publish-all.sh <version> [tag]"
  exit 1
fi

# Check if version argument is provided
if [ -z "$1" ]; then
  echo "Error: Version argument is required"
  echo "Usage: ./scripts/publish-all.sh <version> [tag]"
  echo "Example: ./scripts/publish-all.sh 0.1.0-beta.5"
  echo "Example: ./scripts/publish-all.sh 0.1.0-beta.5 beta"
  exit 1
fi

# Strip leading 'v' if present since npm package versions don't use 'v' prefix
VERSION="${1#v}"
NPM_TAG="${2:-}"

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

# Determine npm dist-tag
BETA_TAG=""
if [ -n "$NPM_TAG" ]; then
  BETA_TAG="--tag $NPM_TAG"
  echo "Publishing with tag: $NPM_TAG"
else
  echo "Publishing as latest"
fi
echo ""

# Publish <dir>'s package unless that exact version is already on the registry,
# so a rerun after a partial publish resumes instead of failing on a 403
# ("cannot publish over the previously published version").
publish_pkg() {
  local dir="$1" name published
  name=$(node -p "require('./$dir/package.json').name")
  published=$(npm view "$name@$VERSION" version 2>/dev/null || true)
  if [ -n "$published" ]; then
    echo "✓ $name@$VERSION already published — skipping"
    return 0
  fi
  ( cd "$dir" && npm publish --access public $BETA_TAG --provenance )
  echo "✓ $name published"
}

# Publish platform packages first, then the main package last.
for platform in "${PLATFORMS[@]}"; do
  echo "Publishing $platform..."
  publish_pkg "$platform"
done

echo "Publishing main package icp-cli..."
publish_pkg "icp-cli"

echo ""
echo "All packages published successfully!"
