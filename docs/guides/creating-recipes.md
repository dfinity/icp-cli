# Creating Recipes

Recipes are reusable build templates that you can create to encode your team's build conventions or share with the community.

## Recipe File Structure

A recipe is a YAML file with Handlebars templating. The file extension should be `.hb.yaml`:

```yaml
# recipes/my-recipe.hb.yaml
canister:
  name: {{configuration.name}}
  build:
    steps:
      - type: script
        commands:
          - echo "Building {{configuration.name}}..."
```

## Basic Recipe Example

A simple recipe for optimized Rust builds:

```yaml
# recipes/optimized-rust.hb.yaml
canister:
  name: {{configuration.name}}
  build:
    steps:
      - type: script
        commands:
          - cargo build --package {{configuration.package}} --target wasm32-unknown-unknown --release
          - ic-wasm target/wasm32-unknown-unknown/release/{{configuration.package}}.wasm -o "$ICP_WASM_OUTPUT_PATH" shrink
```

Usage:

```yaml
canisters:
  - name: backend
    recipe:
      type: ./recipes/optimized-rust.hb.yaml
      configuration:
        name: backend
        package: my-backend-crate
```

## Template Syntax

Recipes use [Handlebars](https://handlebarsjs.com/) templating:

### Variables

Access configuration parameters:

```yaml
canister:
  name: {{configuration.name}}
  build:
    steps:
      - type: script
        commands:
          - cargo build --package {{configuration.package}}
```

### Conditionals

Use `{{#if}}` for optional configuration:

```yaml
canister:
  name: {{configuration.name}}
  build:
    steps:
      - type: script
        commands:
          {{#if configuration.optimize}}
          - cargo build --release --target wasm32-unknown-unknown
          - ic-wasm target/wasm32-unknown-unknown/release/{{configuration.package}}.wasm -o "$ICP_WASM_OUTPUT_PATH" shrink
          {{else}}
          - cargo build --target wasm32-unknown-unknown
          - cp target/wasm32-unknown-unknown/debug/{{configuration.package}}.wasm "$ICP_WASM_OUTPUT_PATH"
          {{/if}}
```

### Loops

Use `{{#each}}` for dynamic lists:

```yaml
canister:
  name: {{configuration.name}}
  build:
    steps:
      - type: script
        commands:
          {{#each configuration.prebuild_commands}}
          - {{this}}
          {{/each}}
          - cargo build --target wasm32-unknown-unknown --release
```

### Default Values

Use `{{#if}}` with `{{else}}` for defaults:

```yaml
canister:
  name: {{configuration.name}}
  settings:
    compute_allocation: {{#if configuration.compute_allocation}}{{configuration.compute_allocation}}{{else}}0{{/if}}
```

### Nested Configuration

Access nested objects:

```yaml
canister:
  name: {{configuration.name}}
  {{#if configuration.settings}}
  settings:
    {{#if configuration.settings.compute_allocation}}
    compute_allocation: {{configuration.settings.compute_allocation}}
    {{/if}}
    {{#if configuration.settings.memory_allocation}}
    memory_allocation: {{configuration.settings.memory_allocation}}
    {{/if}}
  {{/if}}
```

## Recipe with Sync Steps

Include post-deployment operations:

```yaml
canister:
  name: {{configuration.name}}
  build:
    steps:
      - type: script
        commands:
          - npm run build
  sync:
    steps:
      - type: assets
        source: {{configuration.source}}
        target: /
```

## Testing Recipes

Test your recipe by viewing the expanded configuration:

```bash
icp project show
```

This shows exactly what your recipe produces after template expansion.

Verify it works end-to-end:

```bash
icp build
icp deploy
```

## Sharing Recipes

### Within a Team

Store recipes in your project's `recipes/` directory and reference with relative paths:

```yaml
recipe:
  type: ./recipes/my-recipe.hb.yaml
  configuration:
    name: my-canister
```

### Across Projects

Host on a web server or GitHub and reference with URL and sha256 hash:

```yaml
recipe:
  type: https://example.com/recipes/my-recipe.hb.yaml
  sha256: <sha256-hash-of-file>
  configuration:
    name: my-canister
```

Generate the hash:

```bash
sha256sum recipes/my-recipe.hb.yaml
```

### Publishing to the Registry

To contribute recipes to the official registry at [github.com/dfinity/icp-cli-recipes](https://github.com/dfinity/icp-cli-recipes):

1. Fork the repository
2. Add your recipe following the contribution guidelines
3. Submit a pull request

## Recipe Examples

### Frontend Build with Asset Upload

```yaml
canister:
  name: {{configuration.name}}
  build:
    steps:
      - type: script
        commands:
          - npm install
          - npm run build
  sync:
    steps:
      - type: assets
        source: {{configuration.source}}
        target: /
```

### Multi-Step Build Process

```yaml
canister:
  name: {{configuration.name}}
  build:
    steps:
      - type: script
        commands:
          # Install dependencies
          {{#each configuration.dependencies}}
          - {{this}}
          {{/each}}
          # Run build
          - {{configuration.build_command}}
          # Optimize WASM
          {{#if configuration.optimize}}
          - ic-wasm {{configuration.wasm_path}} -o "$ICP_WASM_OUTPUT_PATH" shrink
          {{else}}
          - cp {{configuration.wasm_path}} "$ICP_WASM_OUTPUT_PATH"
          {{/if}}
```

## Best Practices

- **Keep recipes focused** — One recipe per build pattern
- **Document configuration options** — Include comments or a README
- **Provide sensible defaults** — Use conditionals to make options optional
- **Test thoroughly** — Verify recipes work across different projects
- **Version carefully** — Use semantic versioning for published recipes

## Next Steps

- [Using Recipes](using-recipes.md) — Apply recipes in your projects
- [Recipes Concept](../concepts/recipes.md) — Understand how recipes work

[Browse all documentation →](../index.md)
