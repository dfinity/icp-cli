#!/bin/bash

#
# Generate CLI documentation script
# This script generates markdown documentation for all CLI commands
#

set -e

echo "Generating CLI documentation..."

# Generate the full CLI documentation
echo "Building the CLI..."
cargo build

echo "Generating markdown documentation..."
$(git rev-parse --show-toplevel)/target/debug/icp --markdown-help > $(git rev-parse --show-toplevel)/docs/reference/cli.md

echo "Documentation generated successfully at docs/reference/cli.md"

echo ""
echo "✅ Documentation generation complete!"
echo ""
