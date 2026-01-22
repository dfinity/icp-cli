#!/usr/bin/env bash

# Skip if large.wasm already exists and is larger than 3MB (3000000 bytes)
if [[ -f large.wasm ]] && [[ $(stat -f%z large.wasm 2>/dev/null || stat -c%s large.wasm 2>/dev/null) -gt 3000000 ]]; then
  echo "large.wasm already exists and is >3MB, skipping generation"
  exit 0
fi

# Create a WAT file with a large data segment
{
  echo '(module'
  echo '  (memory 64)'
  printf '  (data (i32.const 0) "'
  dd if=/dev/zero bs=1 count=3000000 2>/dev/null | tr '\0' 'A'
  echo '")'
  echo ')'
} > large.wat

# Convert to Wasm
wasm-tools parse large.wat -o large.wasm
