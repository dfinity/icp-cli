# Versioned Documentation Setup

This document explains how the versioned documentation system works for the ICP CLI.

## Overview

The documentation site supports multiple versions simultaneously:
- `https://dfinity.github.io/icp-cli/` → Redirects to latest version
- `https://dfinity.github.io/icp-cli/0.1/` → Version 0.1 docs
- `https://dfinity.github.io/icp-cli/main/` → Main branch docs (preview)

Users can switch between versions using the version switcher dropdown in the header.

## Architecture

### Directory Structure (gh-pages branch)

```
├── index.html       # Redirects to latest version (or /main/ if no releases)
├── versions.json    # List of available versions
├── main/            # Main branch docs (always updated)
├── 0.1/             # Version 0.1 docs
├── 0.2/             # Version 0.2 docs
└── ...
```

### Workflow Triggers

The workflow [`.github/workflows/docs.yml`](.github/workflows/docs.yml) handles deployment:

| Trigger | Action |
|---------|--------|
| Tag `v*` (e.g., `v0.1.0`) | Deploys to `/0.1/` (major.minor) |
| Branch `docs/v*` (e.g., `docs/v0.1`) | Updates `/0.1/` (for cherry-picks) |
| Push to `main` | Deploys to `/main/`, updates root `index.html` and `versions.json` |

### Version Switcher

The component ([VersionSwitcher.astro](../docs-site/src/components/VersionSwitcher.astro)):
- Extracts current version from the URL path at build time
- Fetches `versions.json` at runtime using the configured base prefix
- Shows "dev" badge in local development, "main" badge on main branch
- Shows interactive dropdown with all versions for released docs

## Configuration

### Environment Variables

Set these in the workflow file to configure deployment:

```yaml
env:
  PUBLIC_SITE: https://dfinity.github.io      # GitHub Pages base URL
  PUBLIC_BASE_PREFIX: /icp-cli                # Repository path prefix
```

**For forks**, update both values:
```yaml
env:
  PUBLIC_SITE: https://your-username.github.io
  PUBLIC_BASE_PREFIX: /your-repo-name
```

The `build` job validates these are set before proceeding.

### GitHub Pages Settings

In **Settings → Pages**:
- **Source**: Deploy from a branch
- **Branch**: `gh-pages` / `/ (root)`

### versions.json

Located at [docs-site/versions.json](../docs-site/versions.json). Update when releasing:

```json
{
  "versions": [
    {"version": "0.2", "latest": true},
    {"version": "0.1"}
  ]
}
```

The workflow copies this to gh-pages root and generates `index.html` redirecting to the first entry.

## Common Tasks

### First Deployment (Pre-release)

```bash
git push origin main
# → Deploys to /main/, redirect points to /main/
```

### First Release

```bash
# 1. Deploy docs
git tag v0.1.0
git push origin v0.1.0

# 2. Update versions.json: add {"version": "0.1", "latest": true}
git add docs-site/versions.json
git commit -m "docs: add v0.1 to version list"
git push origin main
```

### Subsequent Releases

```bash
# 1. Deploy docs
git tag v0.2.0
git push origin v0.2.0

# 2. Update versions.json: add 0.2 at top with latest: true, remove latest from 0.1
git add docs-site/versions.json
git commit -m "docs: add v0.2 to version list"
git push origin main
```

**Important**: Push the tag first, then update versions.json to avoid 404s.

### Update Old Version Docs

```bash
git checkout v0.1.0
git checkout -b docs/v0.1
# Make changes
git commit -m "docs: fix typo in v0.1"
git push origin docs/v0.1
```

Or push a patch tag (`v0.1.1`) — it deploys to the same `/0.1/` directory.

### Beta Versions

Create a docs branch with the full version:

```bash
git checkout -b docs/v0.2.0-beta.5
git push origin docs/v0.2.0-beta.5
# → Deploys to /0.2.0-beta.5/
```

Don't add beta versions to `versions.json` — they won't appear in the switcher.

## Local Testing

Run the test script to simulate the full deployment locally:

```bash
./docs-site/test-version-switcher.sh
```

This builds multiple versions and serves them at `http://localhost:4321/icp-cli/`.

## Troubleshooting

| Problem | Solution |
|---------|----------|
| Workflow fails with "environment variable is not set" | Add `PUBLIC_SITE` and `PUBLIC_BASE_PREFIX` to workflow `env:` section |
| Version switcher shows "Failed to load versions" | Check `versions.json` exists at gh-pages root, check browser console |
| Tag pushed but docs not deployed | Verify tag matches `v*` pattern, check workflow logs |
| New version not in switcher | Push `versions.json` update to main after adding the version |
| Redirect not working | Check `index.html` in gh-pages, clear browser cache |
| Deployment replaces other versions | Verify `keep_files: true` and correct `destination_dir` in workflow |
