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
- Regenerate when manifest types change: `./scripts/generate-config-schema.sh`

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
