#!/bin/bash
set -e

echo "Validating WASM at: $ICP_WASM_PATH"
echo "For canister: $ICP_CANISTER_ID"
echo "Canister directory: $ICP_CANISTER_PATH"

# Check WASM size
SIZE=$(stat -f%z "$ICP_WASM_PATH" 2>/dev/null || stat -c%s "$ICP_WASM_PATH")
MAX_SIZE=$((2 * 1024 * 1024))  # 2MB

if [ $SIZE -gt $MAX_SIZE ]; then
    echo "ERROR: WASM size ($SIZE bytes) exceeds maximum ($MAX_SIZE bytes)"
    exit 1
fi

echo "✓ WASM validation passed (size: $SIZE bytes)"
