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

# Fix clap-markdown behavior where it prepends command path to override_usage.
# The token subcommands use override_usage to show the full usage including the
# TOKEN argument from the parent command, but clap-markdown also prepends "icp token",
# resulting in "icp token icp token ...". This sed command removes the duplication.
sed -i '' 's/icp token icp token/icp token/g' $(git rev-parse --show-toplevel)/docs/reference/cli.md

echo "Documentation generated successfully at docs/reference/cli.md"

echo ""
echo "✅ Documentation generation complete!"
echo ""
