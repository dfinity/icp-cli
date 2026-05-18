#!/bin/bash
set -euo pipefail
sudo apt-get update && sudo apt-get install -y softhsm2 pipx
pipx install mitmproxy
