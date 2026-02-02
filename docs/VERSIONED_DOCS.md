# Versioned Documentation Setup

This document explains how the versioned documentation system works for the ICP CLI.

## Overview

The documentation site supports multiple versions simultaneously:
- `https://dfinity.github.io/icp-cli/` → Redirects to latest version
- `https://dfinity.github.io/icp-cli/0.1/` → Version 0.1 docs
- `https://dfinity.github.io/icp-cli/0.2/` → Version 0.2 docs

Users can switch between versions using the version switcher in the header.

## Architecture

### GitHub Pages Deployment

The site uses the `gh-pages` branch for deployment via the `peaceiris/actions-gh-pages` action, which allows multiple versions to coexist in subdirectories.

### Directory Structure

```
gh-pages branch:
├── index.html       # Redirects to latest version (or main if no releases)
├── versions.json    # List of available versions
├── main/           # Main branch docs (always updated, for preview)
│   └── (full site)
├── 0.1/            # Version 0.1 docs
│   └── (full site)
├── 0.2/            # Version 0.2 docs
│   └── (full site)
└── ...
```

### Workflow

**[.github/workflows/docs.yml](.github/workflows/docs.yml)** handles all deployment:

1. **Tag triggers** (`v*`):
   - Pushing tag `v0.1.0` automatically deploys docs to `/0.1/` (major.minor)
   - Pushing tag `v0.2.0` automatically deploys docs to `/0.2/`

2. **Branch triggers** (`docs/v*`):
   - Pushing to `docs/v0.1` updates docs at `/0.1/`
   - Useful for cherry-picking fixes to older versions

3. **Main branch**:
   - Deploys main branch docs to `/main/` (for preview/development)
   - Updates `index.html` (redirect to latest version, or `main` if no releases)
   - Updates `versions.json` (list of available versions)

**Jobs:**
- `build`: Validates docs build on all changes
- `publish-root-files`: Updates index.html and versions.json (runs on `main`)
- `publish-main-docs`: Deploys main branch docs to `/main/` (runs on `main`)
- `publish-versioned-docs`: Publishes versioned docs (runs on tags or `docs/v*` branches)

## Initial Setup

### 1. Configuration

The documentation system is fully configurable via environment variables in the workflow file [`.github/workflows/docs.yml`](.github/workflows/docs.yml).

**Required environment variables:**
- `PUBLIC_SITE`: The GitHub Pages base URL (e.g., `https://dfinity.github.io`)
- `PUBLIC_BASE_PREFIX`: The repository path prefix (e.g., `/icp-cli`)

**For the main dfinity/icp-cli repository:**
```yaml
env:
  PUBLIC_SITE: https://dfinity.github.io
  PUBLIC_BASE_PREFIX: /icp-cli
```

**For forks:**
```yaml
env:
  PUBLIC_SITE: https://your-username.github.io
  PUBLIC_BASE_PREFIX: /your-repo-name
```

These values are set at the workflow level and used by all build jobs. No hardcoded defaults exist in the source code.

**Validation:** The `build` job validates that both variables are set before building the docs. If either variable is missing, the workflow fails early with a clear error message. Since all publish jobs depend on `build` (via `needs: build`), this single validation point protects the entire pipeline.

### 2. GitHub Pages Settings

Go to **Settings → Pages** in the GitHub repository and configure:

- **Source**: Deploy from a branch
- **Branch**: `gh-pages`
- **Folder**: `/ (root)`

⚠️ **Important**: Change from "GitHub Actions" to "Deploy from a branch" for this to work.

### 3. Version List Configuration

The version list is stored in [docs-site/versions.json](../docs-site/versions.json).

**Before first release** (empty array):
```json
{
  "versions": []
}
```
- Redirect goes to `/main/` (main branch docs)
- Version switcher shows "main" badge

**After releases** (add versions, newest first):
```json
{
  "versions": [
    {"version": "0.1", "latest": true}
  ]
}
```

The workflow automatically:
- Deploys this file to the root of gh-pages
- Reads the **first version** in the list (newest)
- Generates `index.html` to redirect to that version (or `main` if empty)

**You only need to update this file when releasing new versions** - the redirect is automated.

### 4. First Deployment

**Before first release:**

1. **Push to main**:
   ```bash
   git push origin main
   ```
   This deploys:
   - Root files (index.html → redirects to `/main/`, versions.json)
   - Main branch docs to `/main/`

2. **Verify pre-release state**:
   - `https://dfinity.github.io/icp-cli/` → redirects to `/main/`
   - `https://dfinity.github.io/icp-cli/main/` → loads main branch docs
   - Version switcher shows "main" badge

**First release (v0.1.0):**

1. **Push release tag** (deploys docs to `/0.1/`):
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

2. **Update versions.json and push to main** (updates redirect):
   ```bash
   # Edit docs-site/versions.json to add:
   # {"version": "0.1", "latest": true}

   git add docs-site/versions.json
   git commit -m "docs: add v0.1 to version list"
   git push origin main
   ```

3. **Verify release**:
   - `https://dfinity.github.io/icp-cli/` → redirects to `/0.1/`
   - `https://dfinity.github.io/icp-cli/0.1/` → loads v0.1 docs
   - `https://dfinity.github.io/icp-cli/main/` → still accessible for preview
   - Version switcher shows "0.1 (latest)"

## Main Branch Docs

The `/main/` path always contains docs built from the main branch:

**Purpose:**
- Provides live docs before the first release
- Allows contributors to preview upcoming documentation changes
- Useful for testing docs updates before creating a release

**Behavior:**
- Always deployed when pushing to main
- Redirect points to `/main/` when versions.json is empty (pre-release)
- Redirect points to latest version after first release
- Version switcher shows "main" badge (non-interactive)
- Remains accessible at `/main/` even after releases

**Access:**
- Direct: `https://dfinity.github.io/icp-cli/main/`
- Redirect (pre-release only): `https://dfinity.github.io/icp-cli/`

## Releasing a New Version

When releasing v0.2.0:

### 1. Deploy

**Important**: Deploy docs FIRST, then update redirect to avoid 404s.

```bash
# Step 1: Push release tag (deploys v0.2 docs to /0.2/)
git tag v0.2.0
git push origin v0.2.0

# Step 2: Edit docs-site/versions.json to add v0.2 at top (newest first):
# {"version": "0.2", "latest": true}
# And update v0.1 to remove latest flag (or omit it)

# Step 3: Push to main (redirect now points to /0.2/, which exists)
git add docs-site/versions.json
git commit -m "docs: add v0.2 to version list"
git push origin main
```

### 2. Verify

- `https://dfinity.github.io/icp-cli/` → redirects to `/0.2/`
- `https://dfinity.github.io/icp-cli/0.2/` → loads v0.2 docs
- `https://dfinity.github.io/icp-cli/0.1/` → still works (old version)
- Version switcher shows both versions, with 0.2 marked as latest

## Updating Existing Version Docs

To update docs for an already-released version (e.g., cherry-pick bug fixes):

### Option 1: Create docs branch from existing tag

```bash
git checkout v0.1.0
git checkout -b docs/v0.1
# Make changes to docs
git commit -m "docs: fix typo in v0.1 docs"
git push origin docs/v0.1
```

The workflow will automatically rebuild and redeploy to `/0.1/`.

### Option 2: Push a new patch release tag

```bash
git tag v0.1.1
git push origin v0.1.1
```

Since patch version is stripped (`v0.1.1` → `0.1`), this updates the same `/0.1/` directory.

Both approaches preserve other version directories due to `keep_files: true`.

## Beta/Pre-release Versions

For beta versions, you can either:

### Option 1: Don't deploy beta docs

Just skip deploying until stable release. Beta users can build docs locally.

### Option 2: Deploy beta to its own version

Create a docs branch with full version:

```bash
git checkout v0.2.0-beta.5
git checkout -b docs/v0.2.0-beta.5
git push origin docs/v0.2.0-beta.5
```

This deploys to `/0.2.0-beta.5/` (full version, not just major.minor).

**Do not add beta versions to versions.json** - they won't appear in the version switcher.

## Version Switcher Component

The version switcher ([docs-site/src/components/VersionSwitcher.astro](docs-site/src/components/VersionSwitcher.astro)):
- Shows "main" badge when viewing main branch docs or no releases exist
- Shows "dev" badge in local development
- Shows version dropdown for released versions
- Reads current version from Cargo.toml at build time
- Fetches `/icp-cli/versions.json` at runtime
- Shows "(latest)" label for the latest version
- Highlights current version with checkmark

## How It Works: Tag vs Branch

**When you push a tag** (e.g., `v0.1.0`):
1. Workflow extracts major.minor: `v0.1.0` → `0.1`
2. Builds docs with base path `/icp-cli/0.1/`
3. Deploys to gh-pages branch at `/0.1/`

**When you push to docs branch** (e.g., `docs/v0.1`):
1. Workflow extracts version: `docs/v0.1` → `0.1`
2. Builds docs with base path `/icp-cli/0.1/`
3. Deploys to gh-pages branch at `/0.1/` (replaces existing)

**When you push to main**:
1. Deploys main branch docs to `/main/`
2. Updates `index.html` at root (redirect)
3. Updates `versions.json` at root (version list)
4. Uses `keep_files: true` to preserve version directories

## Troubleshooting

### Workflow fails with "environment variable is not set"
- Error: `❌ Error: PUBLIC_SITE environment variable is not set` or `PUBLIC_BASE_PREFIX environment variable is not set`
- Cause: Required environment variables are missing from workflow configuration
- Fix: Add both `PUBLIC_SITE` and `PUBLIC_BASE_PREFIX` to the `env:` section at the top of [`.github/workflows/docs.yml`](.github/workflows/docs.yml)
- See [Configuration](#1-configuration) section for examples

### Version switcher shows "Failed to load versions"
- Check that `versions.json` was deployed to `/icp-cli/versions.json`
- Check browser console for fetch errors
- Verify gh-pages branch contains `versions.json` at root

### Tag pushed but docs not deployed
- Check GitHub Actions workflow ran successfully
- Verify tag matches pattern `v*` (e.g., `v0.1.0`, not `0.1.0`)
- Check workflow logs for errors

### New version not appearing in switcher
- Verify `versions.json` was updated in the workflow
- Check that main branch was pushed after updating workflow
- Verify gh-pages branch contains updated `versions.json`

### Redirect not working
- Check `index.html` in gh-pages branch root
- Verify the redirect URL matches the deployed version path
- Clear browser cache

### Deployment replaces other versions
- Verify workflow uses `keep_files: true` in peaceiris/actions-gh-pages
- Check that `destination_dir` is set correctly for versioned deployments

## Quick Reference

**Pre-release (main branch only)**:
```bash
# versions.json is empty - redirect goes to /main/
git push origin main
# → https://dfinity.github.io/icp-cli/ redirects to /main/
```

**First release (v0.1.0)**:
```bash
# 1. Push tag (deploys docs to /0.1/)
git tag v0.1.0
git push origin v0.1.0

# 2. Edit docs-site/versions.json, add: {"version": "0.1", "latest": true}
# 3. Push to main (updates redirect)
git add docs-site/versions.json
git commit -m "docs: add v0.1 to version list"
git push origin main
```

**Subsequent release (v0.2.0)**:
```bash
# 1. Push tag (deploys docs to /0.2/)
git tag v0.2.0
git push origin v0.2.0

# 2. Edit docs-site/versions.json, add v0.2 at top, update v0.1
# 3. Push to main (updates redirect)
git add docs-site/versions.json
git commit -m "docs: add v0.2 to version list"
git push origin main
```

**Update old version docs**:
```bash
git checkout v0.1.0
git checkout -b docs/v0.1
# Make doc changes
git commit -m "docs: update v0.1 docs"
git push origin docs/v0.1
```

That's it! Tags automatically deploy new versions, branches update existing versions.
