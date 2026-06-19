#!/bin/bash
set -euo pipefail
# Retry + bound each fetch: azure.archive.ubuntu.com occasionally stalls a
# connection for minutes, so fail fast (Timeout) and retry instead of hanging.
# --no-install-recommends drops an unused doc toolchain (mkdocs, sphinx,
# tornado, livereload, libjs-*) that pipx/softhsm2 pull in via recommends and
# that we never use; python3-venv (needed by pipx) is already on the runner.
APT_OPTS="-o Acquire::Retries=3 -o Acquire::http::Timeout=30"
sudo apt-get update $APT_OPTS
sudo apt-get install -y --no-install-recommends $APT_OPTS softhsm2 pipx
pipx install mitmproxy
