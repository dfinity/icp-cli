#!/bin/bash

set -e

# Start the HTTP server in the background
python3 -m http.server 8080 &
SERVER_PID=$!

# Give the server a moment to start
sleep 1

# Function to cleanup server on exit
cleanup() {
    echo "Stopping HTTP server (PID: $SERVER_PID)"
    kill $SERVER_PID 2>/dev/null || true
    wait $SERVER_PID 2>/dev/null || true
}

# Ensure cleanup happens on script exit
trap cleanup EXIT

# Run icp project show
icp project show
