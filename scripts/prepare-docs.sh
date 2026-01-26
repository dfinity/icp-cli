#!/usr/bin/env bash
set -euo pipefail

# Prepare documentation for Starlight build
#
# This script bridges the gap between plain Markdown source files (in docs/)
# and the Astro/Starlight documentation site (in docs-site/). It runs automatically
# during the build process but can also be run manually for testing.
#
# What it does:
# 1. Copies docs/ to docs-site/.docs-temp/ (excluding schemas directory and README files)
# 2. Adjusts relative paths and strips .md extensions for Starlight's clean URLs
# 3. Extracts page titles from H1 headings and adds YAML frontmatter
# 4. Removes H1 headings from content to prevent duplicate titles on the site
#
# Why this approach?
# - Keeps source docs plain Markdown with .md extensions (GitHub-friendly)
# - Build-time transformation creates clean URLs for the documentation site
# - Single source of truth for documentation content
# - Cross-platform compatible (works on macOS and Linux)
#
# Usage:
#   ./prepare-docs.sh                           # Uses default paths
#   ./prepare-docs.sh <source-dir> <target-dir> # Uses custom paths
#
# Example:
#   ./prepare-docs.sh docs docs-site/.docs-temp

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Use provided directories or defaults
SOURCE_DIR="${1:-$PROJECT_ROOT/docs}"
TARGET_DIR="${2:-$PROJECT_ROOT/docs-site/.docs-temp}"

echo "Preparing documentation..."
echo "  Source: $SOURCE_DIR"
echo "  Target: $TARGET_DIR"

# Step 1: Copy docs to target directory (excluding schemas and READMEs)
echo ""
echo "Step 1: Copying documentation files..."
rm -rf "$TARGET_DIR"
mkdir -p "$TARGET_DIR"
rsync -a --exclude='schemas/' --exclude='README.md' --exclude='*/README.md' "$SOURCE_DIR/" "$TARGET_DIR/"
echo "✓ Files copied"

# Step 2: Fix markdown links for Starlight's directory structure
echo ""
echo "Step 2: Fixing markdown links..."
find "$TARGET_DIR" -name "*.md" -type f | while read -r file; do
  # Adjust relative paths and strip .md extensions for Starlight's clean URLs

  basename_file=$(basename "$file")
  dirname_file=$(dirname "$file")
  parent_dirname=$(basename "$dirname_file")

  # For index.md files, only strip .md extensions and add trailing slashes
  if [[ "$basename_file" == "index.md" ]]; then
    sed -i.bak -E 's|\]\(([^:)]+)\.md\)|\]\(\1/\)|g' "$file"
    rm "${file}.bak"
    continue
  fi

  # For root-level files (tutorial.md -> /tutorial/)
  if [[ "$parent_dirname" == ".docs-temp" ]]; then
    # Links to subdirectories: guides/file.md -> ../guides/file/
    sed -i.bak -E 's|\]\(([^/)]+)/([^/)]+)\.md\)|\]\(../\1/\2/\)|g' "$file"
    # Links to root index: index.md -> ../ (root is served at /, not /index)
    sed -i.bak -E 's|\]\(index\.md\)|\]\(../\)|g' "$file"
    # Links to other root-level pages: file.md -> ../file/
    sed -i.bak -E 's|\]\(([^/)(]+)\.md\)|\]\(../\1/\)|g' "$file"
    rm "${file}.bak"
  else
    # For files in subdirectories (guides/using-recipes.md -> /guides/using-recipes/)
    # Links to category index: ../concepts/index.md -> ../../concepts/ (not ../../concepts/index/)
    sed -i.bak -E 's|\]\(\.\./([^/)]+)/index\.md\)|\]\(../../\1/\)|g' "$file"
    # Links to other categories: ../concepts/file.md -> ../../concepts/file/
    sed -i.bak -E 's|\]\(\.\./([^/)]+)/([^/)]+)\.md\)|\]\(../../\1/\2/\)|g' "$file"
    # Links to root index: ../index.md -> ../../ (root is served at /, not /index)
    sed -i.bak -E 's|\]\(\.\./index\.md\)|\]\(../../\)|g' "$file"
    # Links up to other root pages: ../file.md -> ../../file/
    sed -i.bak -E 's|\]\(\.\./([^/)(]+)\.md\)|\]\(../../\1/\)|g' "$file"
    # Same-directory links: file.md -> ../file/
    sed -i.bak -E 's|\]\(([^/.)][^/)]*)\.md\)|\]\(../\1/\)|g' "$file"
    rm "${file}.bak"
  fi
done
echo "✓ Links fixed"

# Step 3: Add frontmatter to files
echo ""
echo "Step 3: Adding frontmatter..."
find "$TARGET_DIR" -name "*.md" -type f | while read -r file; do
  # Skip if file already has frontmatter
  if head -n 1 "$file" | grep -q "^---"; then
    continue
  fi

  # Extract title from first # heading, or use special title for index.md
  if [[ "$(basename "$file")" == "index.md" ]]; then
    title="icp-cli Documentation"
  else
    title=$(grep -m 1 "^# " "$file" | sed 's/^# //' || basename "$file" .md)
  fi

  # Create temporary file with frontmatter
  temp_file="${file}.tmp"
  {
    echo "---"
    echo "title: $title"
    # Add banner to all pages (will be removed once versioning is introduced)
    echo "banner:"
    echo "  content: 'This documentation reflects the latest main branch and may include features not yet in the <a href=\"https://github.com/dfinity/icp-cli/releases\" target=\"_blank\" rel=\"noopener noreferrer\">current beta release</a>. Feedback welcome on the <a href=\"https://forum.dfinity.org/t/first-beta-release-of-icp-cli/60410\" target=\"_blank\" rel=\"noopener noreferrer\">Forum</a> or <a href=\"https://discord.internetcomputer.org\" target=\"_blank\" rel=\"noopener noreferrer\">Discord</a>!'"
    echo "---"
    echo ""
    # Remove the first H1 heading line from content to avoid duplicates
    awk '
      BEGIN { found_h1=0 }
      /^# / && found_h1==0 { found_h1=1; next }
      { print }
    ' "$file"
  } > "$temp_file"

  # Replace original file
  mv "$temp_file" "$file"
done
echo "✓ Frontmatter added"

echo ""
echo "✓ Documentation prepared successfully!"
