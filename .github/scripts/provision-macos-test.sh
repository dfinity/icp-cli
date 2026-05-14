#!/bin/bash
set -euo pipefail
brew install softhsm mitmproxy
# The new macOS runner image ships with Homebrew's rustup formula, which places
# /opt/homebrew/bin/cargo as a rustup-init shim. brew install above can activate
# it, shadowing the rustup-managed cargo. Unlink it so setup-rust-toolchain wins.
brew unlink rust rustup || true
