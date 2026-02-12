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

## Writing Guidelines

- Use "canister environment variables" (not just "environment variables") when referring to runtime variables stored in canister settings — this distinguishes them from shell/build environment variables
- Verify code examples and CLI commands work before committing; explain non-obvious flags
- Link to anchors on other pages rather than duplicating content (e.g., `[Custom Variables](../reference/environment-variables.md#custom-variables)`)
- Link to external tools rather than duplicating their documentation

## Link Formatting

Source documentation in `docs/` must work in two contexts:

1. **GitHub**: Renders Markdown directly with `.md` extensions
2. **Starlight docs site**: `scripts/prepare-docs.sh` transforms links to clean URLs

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

The `prepare-docs.sh` script handles the transformation to Starlight's URL structure. If you add new link patterns, verify they work by building the docs site locally with `cd docs-site && npm run build`.
