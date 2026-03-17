# Versioned Documentation Setup

This document explains how the versioned documentation system works for the ICP CLI.

## Overview

The documentation site supports multiple versions simultaneously:
- `https://cli.icp.build/` → Redirects to latest version
- `https://cli.icp.build/0.1/` → Version 0.1 docs
- `https://cli.icp.build/0.2/` → Version 0.2 docs
- `https://cli.icp.build/main/` → Main branch docs (preview)

Users can switch between versions using the version switcher dropdown in the header.

## Architecture

The site is hosted on an IC asset canister (`ak73b-maaaa-aaaad-qlbgq-cai`) and served via the custom domain `cli.icp.build`.

### Deployment branch

All built assets live on the `docs-deployment` branch:

```
├── index.html                      # Redirects to latest version (or /main/ if no releases)
├── versions.json                   # List of available versions
├── icp.yaml                        # IC asset canister config
├── .ic-assets.json5                # Asset routing/headers config
├── .icp/data/mappings/ic.ids.json  # Canister ID mapping
├── .well-known/ic-domains          # Custom domain verification
├── main/                           # Main branch docs (always updated)
├── 0.1/                            # Version 0.1 docs
├── 0.2/                            # Version 0.2 docs
└── ...
```

### Two-workflow pipeline

**`.github/workflows/docs.yml`** — builds docs and pushes to `docs-deployment`:

| Trigger | Action |
|---------|--------|
| Tag `v*` (e.g., `v0.2.0`) | Deploys to `/0.2/` (major.minor only) |
| Branch `docs/v*` (e.g., `docs/v0.1`) | Updates `/0.1/` (for cherry-picks / fixes to old versions) |
| Push to `main` | Deploys to `/main/`, updates root `index.html` and `versions.json` |

**`.github/workflows/docs-deploy.yml`** — deploys `docs-deployment` to the IC:

- Triggers automatically whenever `docs.yml` pushes to `docs-deployment`
- Runs `icp deploy -e ic docs` using the `DFX_IDENTITY_DESIGN_TEAM` secret
- Requires the **IC mainnet** GitHub environment

### Version Switcher

The component ([VersionSwitcher.astro](../docs-site/src/components/VersionSwitcher.astro)):
- Extracts the current version from the URL path at build time
- Fetches `/versions.json` at runtime
- Shows "dev" badge in local development, "main" badge on main branch
- Shows interactive dropdown with all versions for released docs

## Configuration

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

The workflow copies this to the `docs-deployment` root and generates `index.html` redirecting to the version marked `latest: true`.

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

**Important**: Push the tag first, then update `versions.json` to avoid 404s.

### Update Old Version Docs

```bash
git checkout v0.1.0
git checkout -b docs/v0.1
# Make changes
git commit -m "docs: fix typo in v0.1"
git push origin docs/v0.1
# → Rebuilds and redeploys /0.1/
```

Or push a patch tag (`v0.1.1`) — it deploys to the same `/0.1/` directory.

### Beta Versions

Create a docs branch with the full version:

```bash
git checkout -b docs/v0.2.0-beta.5
git push origin docs/v0.2.0-beta.5
# → Deploys to /0.2.0-beta.5/
```

Don't add beta versions to `versions.json` — they won't appear in the version switcher.

## Local Testing

Run the test script to simulate multi-version builds locally:

```bash
./docs-site/test-version-switcher.sh
```

This builds multiple versions and serves them at `http://localhost:4321`.

## Troubleshooting

| Problem | Solution |
|---------|----------|
| Version switcher shows "Failed to load versions" | Check `versions.json` exists at `docs-deployment` root, check browser console |
| Tag pushed but docs not deployed | Verify tag matches `v*` pattern, check workflow logs |
| New version not in switcher | Push `versions.json` update to main after adding the version |
| Deployment replaces other versions | Verify `keep_files: true` and correct `destination_dir` in the `docs.yml` workflow |
| Custom domain not resolving | Verify `.well-known/ic-domains` is present on `docs-deployment` and DNS CNAME points to `icp1.io` |
