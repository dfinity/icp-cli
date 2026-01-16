#!/usr/bin/env bash
set -euo pipefail

# Script to validate all examples in the examples/ directory
# This script can be run locally or in CI

# Add icp binary to PATH (assumes it's in target/debug)
export PATH="$(pwd)/target/debug:$PATH"
echo "icp version: $(icp --version)"
echo "icp path: $(which icp)"

# Get all example directories
for example_dir in examples/*/; do
  # Skip if not a directory
  if [ ! -d "$example_dir" ]; then
    continue
  fi
  
  example_name=$(basename "$example_dir")
  echo ""
  echo "=========================================="
  echo "Validating example: $example_name"
  echo "=========================================="
  
  pushd "$example_dir"
  
  # Check if test.sh exists
  if [ -f "test.sh" ]; then
    echo "Found test.sh, running it..."
    if ! bash test.sh; then
      echo "❌ test.sh failed in $example_name"
      exit 1
    fi
    echo "✅ test.sh passed for $example_name"
  else
    # Run icp project show to validate the project
    echo "Running: icp project show"
    if ! icp project show; then
      echo "❌ Failed to validate project in $example_name"
      exit 1
    fi
    echo "✅ Project validation successful for $example_name"
  fi
  
  popd
done

echo ""
echo "=========================================="
echo "✅ All examples validated successfully!"
echo "=========================================="
