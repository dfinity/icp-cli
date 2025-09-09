#!/bin/bash
#
# Generate JSON Schema for icp.yaml configuration files
#
# This script automatically generates the docs/icp-yaml-schema.json file
# from the Rust type definitions in the codebase.
#

set -e

echo "🔨 Building schema generator..."
cargo build --bin schema-gen --quiet

echo "📋 Generating JSON Schema..."
cargo run --bin schema-gen > $(git rev-parse --show-toplevel)/docs/icp-yaml-schema.json
echo "✅ Schema generation complete!"
echo "📄 Generated file: docs/icp-yaml-schema.json"

