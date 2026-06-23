#!/usr/bin/env bash
set -euo pipefail
# Retry + bound each fetch: azure.archive.ubuntu.com occasionally stalls a
# connection for minutes, so fail fast (Timeout) and retry instead of hanging.
APT_OPTS="-o Acquire::Retries=3 -o Acquire::http::Timeout=30"
sudo apt-get update $APT_OPTS
sudo apt-get install -y $APT_OPTS libdbus-1-dev dbus
