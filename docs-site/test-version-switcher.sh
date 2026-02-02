#!/bin/bash
# Local test script for version switcher - builds all versions for complete testing
# This script simulates the production deployment locally for testing purposes

set -e

cd "$(dirname "$0")"

# Cleanup function to kill the server
cleanup() {
  echo ""
  echo "Shutting down server..."
  if [ -n "$SERVER_PID" ]; then
    kill "$SERVER_PID" 2>/dev/null || true
  fi
  exit 0
}

# Set up trap to catch Ctrl+C and other termination signals
trap cleanup SIGINT SIGTERM

# Configuration - matches production setup
BASE_PREFIX="/icp-cli"
TEST_DIR="dist-test"
TEST_PORT=4321

echo "=================================================="
echo "Building all documentation versions for testing"
echo "=================================================="
echo ""

# Clean everything for a fresh start
echo "Cleaning all previous builds and caches..."
rm -rf "$TEST_DIR" dist .astro
mkdir -p "$TEST_DIR$BASE_PREFIX"

# Set common environment variables
export NODE_ENV=production
export PUBLIC_SITE=http://localhost:4321
export PUBLIC_BASE_PREFIX="$BASE_PREFIX"

# Function to build a version
build_version() {
  local version=$1
  local version_path="${BASE_PREFIX}/${version}/"

  echo ""
  echo "Building version $version..."
  echo "  Base path: $version_path"

  # Clean previous build artifacts to avoid caching issues
  echo "  Cleaning build cache..."
  rm -rf dist .astro

  # Set environment for this specific version
  export PUBLIC_BASE_PATH="$version_path"
  echo "  PUBLIC_BASE_PATH=$PUBLIC_BASE_PATH"
  echo "  Building..."

  npm run build

  # Verify build succeeded
  if [ ! -d "dist" ] || [ ! -f "dist/index.html" ]; then
    echo "❌ Build failed for version $version - dist/index.html not found"
    exit 1
  fi

  # Check what BASE_URL was baked into the build
  BUILT_BASE=$(grep -o 'import\.meta\.env\.BASE_URL[^"]*"[^"]*"' "dist/index.html" | head -1 || echo "not found")
  echo "  Built with BASE_URL: $BUILT_BASE"

  # Copy to test directory
  mkdir -p "$TEST_DIR$BASE_PREFIX/$version"
  cp -r dist/* "$TEST_DIR$BASE_PREFIX/$version/"

  # Verify copy succeeded
  local file_count=$(ls "$TEST_DIR$BASE_PREFIX/$version" | wc -l)
  echo "  Copied $file_count files to test directory"

  echo "✓ Version $version built successfully"
}

# Build each version
build_version "0.1"
build_version "0.2"
build_version "main"

# Generate test versions.json
echo ""
echo "Generating test versions.json..."
cat > "$TEST_DIR$BASE_PREFIX/versions.json" << 'EOF'
{
  "$comment": "Test versions.json for local testing",
  "versions": [
    {
      "version": "0.2",
      "latest": true
    },
    {
      "version": "0.1"
    }
  ]
}
EOF
echo "✓ versions.json created"

# Generate redirect index.html
echo ""
echo "Generating root redirect..."
cat > "$TEST_DIR$BASE_PREFIX/index.html" << EOF
<!doctype html>
<html>
  <head>
    <meta http-equiv="refresh" content="0; url=./0.2/" />
    <meta name="robots" content="noindex" />
    <title>Redirecting to latest version...</title>
  </head>
  <body>
    <p>Redirecting to <a href="./0.2/">latest version</a>...</p>
  </body>
</html>
EOF
echo "✓ index.html created"

echo ""
echo "=================================================="
echo "✓ All versions built successfully!"
echo "=================================================="
echo ""

# Verify the structure
echo "Verifying test structure..."
echo "Directory: $TEST_DIR$BASE_PREFIX/"
ls -lah "$TEST_DIR$BASE_PREFIX/"
echo ""
echo "Checking version subdirectories..."
for version in 0.1 0.2 main; do
    echo ""
    if [ -d "$TEST_DIR$BASE_PREFIX/$version" ]; then
        echo "✓ $version/ exists"
        echo "  Files: $(ls "$TEST_DIR$BASE_PREFIX/$version" | wc -l)"

        if [ -f "$TEST_DIR$BASE_PREFIX/$version/index.html" ]; then
            echo "  index.html: ✓"

            # Check BASE_URL in the built HTML
            BASE_URL_IN_HTML=$(grep -o 'import\.meta\.env\.BASE_URL[^"]*"[^"]*"' "$TEST_DIR$BASE_PREFIX/$version/index.html" | head -1 || echo "not found")
            echo "  BASE_URL in HTML: $BASE_URL_IN_HTML"

            # Check if version switcher is present
            if grep -q "version-switcher" "$TEST_DIR$BASE_PREFIX/$version/index.html"; then
                echo "  VersionSwitcher: ✓ present"

                # Check what's rendered (dev/main badge or button)
                if grep -q "version-button" "$TEST_DIR$BASE_PREFIX/$version/index.html"; then
                    echo "  Renders: version button (interactive dropdown)"
                elif grep -q ">main<" "$TEST_DIR$BASE_PREFIX/$version/index.html"; then
                    echo "  Renders: 'main' badge (⚠️ unexpected for $version)"
                elif grep -q ">dev<" "$TEST_DIR$BASE_PREFIX/$version/index.html"; then
                    echo "  Renders: 'dev' badge (⚠️ unexpected)"
                else
                    echo "  Renders: unknown"
                fi
            else
                echo "  VersionSwitcher: ✗ not found"
            fi
        else
            echo "  index.html: ✗ MISSING"
        fi

        # Check for assets directory
        if [ -d "$TEST_DIR$BASE_PREFIX/$version/_astro" ]; then
            echo "  _astro/ assets: ✓ ($(ls "$TEST_DIR$BASE_PREFIX/$version/_astro" | wc -l) files)"
        else
            echo "  _astro/ assets: ✗ MISSING"
        fi
    else
        echo "✗ $version/ MISSING"
    fi
done
echo ""

# Start Python HTTP server (most reliable for static files)
if ! command -v python3 &> /dev/null; then
    echo "⚠️  Warning: python3 not found. Cannot start server."
    echo "Files are built in: $TEST_DIR$BASE_PREFIX/"
else
    echo "Starting local server with Python..."
    echo "Server starting at: http://localhost:${TEST_PORT}"
    echo ""
    echo "Press Ctrl+C to stop the server when done testing"
    echo ""
    echo "Test URLs:"
    echo "  - http://localhost:${TEST_PORT}$BASE_PREFIX/ (should redirect to 0.2)"
    echo "  - http://localhost:${TEST_PORT}$BASE_PREFIX/0.2/ (version 0.2)"
    echo "  - http://localhost:${TEST_PORT}$BASE_PREFIX/0.1/ (version 0.1)"
    echo "  - http://localhost:${TEST_PORT}$BASE_PREFIX/main/ (main branch)"
    echo ""
    echo "Expected behavior:"
    echo "  ✓ Version 0.2: Button shows 'v0.2', dropdown shows both versions"
    echo "  ✓ Version 0.1: Button shows 'v0.1', dropdown shows both versions"
    echo "  ✓ Main: Shows 'main' badge (no dropdown)"
    echo "  ✓ Clicking versions navigates between them"
    echo "  ✓ Console shows [VersionSwitcher] logs"
    echo ""
    echo "If you see 404s or wrong versions:"
    echo "  1. Check the structure output above"
    echo "  2. Check browser DevTools Console for BASE_URL"
    echo "  3. Check browser DevTools Network tab for asset paths"
    echo ""
    echo "Starting server in 3 seconds..."
    sleep 3

    cd "$TEST_DIR"
    python3 -m http.server ${TEST_PORT} &
    SERVER_PID=$!

    echo ""
    echo "Server is running (PID: $SERVER_PID)"
    echo "Press Ctrl+C to stop the server and exit"
    echo ""

    # Wait for server process
    wait "$SERVER_PID"
fi
