# Documentation Guidelines

## Structure

Documentation follows the Diátaxis framework:

- `docs/tutorial.md` -- Learning-oriented first deployment guide
- `docs/guides/` -- Task-oriented how-to guides
- `docs/concepts/` -- Understanding-oriented explanations
- `docs/reference/` -- Information-oriented technical specifications
- `docs/migration/` -- Migration guides (e.g., from dfx)

## Schema Generation

- Schema is generated in `crates/schema-gen/`
- Referenced in `icp.yaml` files via `# yaml-language-server: $schema=...`
- Regenerate when manifest types change: `./scripts/generate-config-schemas.sh`

## CLI Docs Generation

- CLI reference is in `docs/reference/cli.md`
- Regenerate when commands change: `./scripts/generate-cli-docs.sh`

## Installation Instructions

- **npm is the recommended installation method** for quickstarts, tutorials, and READMEs
- Only `docs/guides/installation.md` should list all installation options
- Follow DRY: other docs should link to the installation guide rather than duplicating instructions
- Consistent ordering: npm (in Quick Install), then Homebrew, then Shell Script (in Alternative Methods)
- When referencing alternatives in other docs, maintain this order: "Homebrew, shell script, ..." (e.g., "See the Installation Guide for Homebrew, shell script, or other options")
- Both `icp-cli` and `ic-wasm` are available as official Homebrew formulas: `brew install icp-cli` and `brew install ic-wasm`

## Docs-Only Fixes for Released Versions

Versioned docs deployments (e.g. `/0.2/`) are controlled by `docs/vX.Y` tags. To fix or improve docs for an already-released version without cutting a new code release:

**Rule: always merge the change to `main` first.** The `docs/vX.Y` tag is only for immediate deployment — when the next patch release is tagged, `sync-docs-tag.yml` resets `docs/vX.Y` to the new release commit. Any commit that exists only on the tag (not in `main`) will be silently lost at that point.

**Workflow:**

```bash
# 1. Merge the fix to main via a normal PR (always required)

# 2. To immediately deploy the fix to /X.Y/ without waiting for a release:
git fetch --tags
git checkout -b temp/docs-fix-vX.Y docs/vX.Y  # start from current tag state
git cherry-pick <commit-sha-from-main>          # pick the merged commit(s)

git tag -f docs/vX.Y HEAD
git push origin refs/tags/docs/vX.Y --force    # triggers re-deploy of /X.Y/

git branch -D temp/docs-fix-vX.Y               # local branch no longer needed
```

The commits remain reachable via the tag — no remote branch is needed.

**On the next release:** `sync-docs-tag.yml` resets `docs/vX.Y` to the release commit. Because the fix was already merged to `main`, the release will contain it, and the reset preserves it automatically.

## Writing Guidelines

- Use "canister environment variables" (not just "environment variables") when referring to runtime variables stored in canister settings — this distinguishes them from shell/build environment variables
- Verify code examples and CLI commands work before committing; explain non-obvious flags
- Link to anchors on other pages rather than duplicating content (e.g., `[Custom Variables](../reference/environment-variables.md#custom-variables)`)
- Link to external tools rather than duplicating their documentation

## Link Formatting

Source documentation in `docs/` must work in two contexts:

1. **GitHub**: Renders Markdown directly with `.md` extensions
2. **Starlight docs site**: A rehype plugin (`docs-site/plugins/rehype-rewrite-links.mjs`) transforms links to clean URLs at build time

**Link format rules:**

- Always use relative paths with `.md` extensions: `[Link](../concepts/file.md)`
- Anchors go after the extension: `[Link](../concepts/file.md#section-name)`
- Never use absolute paths or URLs for internal docs links

**Cross-reference examples:**

```markdown
# From docs/guides/local-development.md:
[Canister Discovery](../concepts/canister-discovery.md)
[Custom Variables](../reference/environment-variables.md#custom-variables)

# From docs/concepts/canister-discovery.md:
[same-directory link](binding-generation.md)
[root-level link](../tutorial.md)
```

The rehype plugin handles the transformation to Starlight's URL structure at build time. If you add new link patterns, verify they work by building the docs site locally with `cd docs-site && npm run build`.
