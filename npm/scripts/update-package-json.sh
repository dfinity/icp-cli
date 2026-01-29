#!/bin/bash

# Ensure script is run from project root
if [ ! -d "icp-cli" ] || [ ! -f "icp-cli/package.json" ]; then
  echo "Error: This script must be run from the project root directory"
  echo "Usage: ./scripts/update-package-json.sh <new-version>"
  exit 1
fi

# Check if version argument is provided
if [ -z "$1" ]; then
  echo "Usage: ./scripts/update-package-json.sh <new-version>"
  echo "Example: ./scripts/update-package-json.sh v0.1.0-beta.5"
  exit 1
fi

# Check if jq is installed
if ! command -v jq &> /dev/null; then
  echo "Error: jq is required but not installed"
  echo "Install with: brew install jq (macOS) or apt-get install jq (Linux)"
  exit 1
fi

# Strip leading 'v' if present since package.json versions don't use 'v' prefix
NEW_VERSION="${1#v}"

echo "Updating all packages to version $NEW_VERSION"

# Update platform packages
for dir in icp-cli-*; do
  if [ -d "$dir" ]; then
    echo "Updating $dir..."
    cd "$dir"
    npm version "$NEW_VERSION" --no-git-tag-version
    cd ..
  fi
done

# Update main package
echo "Updating icp-cli..."
cd icp-cli
npm version "$NEW_VERSION" --no-git-tag-version
cd ..

# Update optionalDependencies in icp-cli/package.json
echo "Updating optionalDependencies in icp-cli/package.json..."
jq --arg version "$NEW_VERSION" \
  '.optionalDependencies = (.optionalDependencies | to_entries | map(.value = $version) | from_entries)' \
  icp-cli/package.json > icp-cli/package.json.tmp
mv icp-cli/package.json.tmp icp-cli/package.json
echo "✓ optionalDependencies updated to $NEW_VERSION"

echo ""
echo "All packages updated to $NEW_VERSION"
