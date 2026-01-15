# Recipes

Recipes are templated build configurations that generate build and sync steps. They reduce boilerplate and encode best practices for common patterns.

## How Recipes Work

A recipe is a [Handlebars](https://handlebarsjs.com/) template that takes configuration parameters and expands into full canister configuration.

```
Recipe Template + Configuration → Expanded Build/Sync Steps
```

### Example

Given this recipe usage:

```yaml
canisters:
  - name: backend
    recipe:
      type: "@dfinity/rust"
      configuration:
        package: my-backend
```

The recipe expands to something like:

```yaml
canisters:
  - name: backend
    build:
      steps:
        - type: script
          commands:
            - cargo build --package my-backend --target wasm32-unknown-unknown --release
            - cp target/wasm32-unknown-unknown/release/my_backend.wasm "$ICP_WASM_OUTPUT_PATH"
```

## Recipe Sources

Recipes can come from three sources:

### Registry (Recommended)

Official recipes from the DFINITY registry:

```yaml
recipe:
  type: "@dfinity/rust"
  configuration:
    package: my-crate
```

Version pinning:

```yaml
recipe:
  type: "@dfinity/rust@v1.0.0"
```

The `@dfinity` prefix resolves to [github.com/dfinity/icp-cli-recipes](https://github.com/dfinity/icp-cli-recipes).

### Local Files

Project-specific recipes:

```yaml
recipe:
  type: ./recipes/my-template.hb.yaml
  configuration:
    param: value
```

### Remote URLs

Recipes hosted anywhere:

```yaml
recipe:
  type: https://example.com/recipes/custom.hb.yaml
  sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
  configuration:
    param: value
```

Always include `sha256` for remote recipes.

## Available Official Recipes

| Recipe | Purpose |
|--------|---------|
| `@dfinity/rust` | Rust canisters with Cargo |
| `@dfinity/motoko` | Motoko canisters |
| `@dfinity/assets` | Asset canisters for static files |
| `@dfinity/prebuilt` | Pre-compiled WASM files |

## Recipe Template Syntax

Recipes use Handlebars templating:

```yaml
# recipes/example.hb.yaml
canister:
  name: {{configuration.name}}
  build:
    steps:
      - type: script
        commands:
          {{#if configuration.optimize}}
          - cargo build --release
          {{else}}
          - cargo build
          {{/if}}
          - cp target/{{configuration.package}}.wasm "$ICP_WASM_OUTPUT_PATH"

  {{#if configuration.settings}}
  settings:
    {{#each configuration.settings}}
    {{@key}}: {{this}}
    {{/each}}
  {{/if}}
```

### Template Variables

- `{{configuration.X}}` — Access configuration parameters
- `{{#if X}}...{{/if}}` — Conditional sections
- `{{#each X}}...{{/each}}` — Loop over arrays or objects
- `{{@key}}` — Current key in an each loop
- `{{this}}` — Current value in an each loop

## Viewing Expanded Configuration

See what recipes expand to:

```bash
icp project show
```

This displays the effective configuration after all recipes are rendered.

## When to Use Recipes

**Use recipes when:**
- Building standard canister types (Rust, Motoko, assets)
- Sharing configurations across multiple canisters
- Encoding team-specific build conventions

**Use direct build steps when:**
- Your build process is unique
- You need fine-grained control
- The overhead of a recipe isn't justified

## Creating Custom Recipes

1. Create a Handlebars template file
2. Define the configuration schema you need
3. Reference it from your `icp.yaml`

Example custom recipe:

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

## Next Steps

- [Using Recipes](../guides/using-recipes.md) — Apply this in practice

[Browse all documentation →](../index.md)
