#!/bin/bash

# Generate CLI documentation script
# This script generates markdown documentation for all CLI commands
#
# TODO This should run as part of a the CI pipeline and throw an error if the file is not up to date

set -e

echo "Generating CLI documentation..."

# Generate the full CLI documentation
echo "Building the CLI..."
cargo build --release

echo "Generating markdown documentation..."
$(git rev-parse --show-toplevel)/target/release/icp --markdown-help > $(git rev-parse --show-toplevel)/docs/cli-reference.md

echo "Documentation generated successfully at docs/cli-reference.md"

echo ""
echo "✅ Documentation generation complete!"
echo ""
