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
      type: "@dfinity/rust@v3.0.0"
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
  type: "@dfinity/rust@v3.0.0"
  configuration:
    package: my-crate
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
| `@dfinity/rust@<version>` | Rust canisters with Cargo |
| `@dfinity/motoko@<version>` | Motoko canisters |
| `@dfinity/asset-canister@<version>` | Asset canisters for static files |
| `@dfinity/prebuilt@<version>` | Pre-compiled WASM files |

## Recipe Template Syntax

Recipes use Handlebars templating:

```yaml
# recipes/example.hb.yaml
build:
  steps:
    - type: script
      commands:
        {{#if optimize}}
        - cargo build --release
        {{else}}
        - cargo build
        {{/if}}
        - cp target/{{configuration.package}}.wasm "$ICP_WASM_OUTPUT_PATH"

```

### Template Variables

icp-cli will essentially render the handlebar template with all the parameters passed
in the configuration section of the recipe.

## Viewing Expanded Configuration

See what recipes expand to:

```bash
icp project show
```

This displays the effective configuration after all recipes are rendered.

## When to Use Recipes

**Use recipes when:**
- Building standard canister types (Rust, Motoko, Asset Canister)
- Sharing configurations across multiple canisters
- Encoding team-specific build conventions

**Use direct build steps when:**
- Your build process is unique
- You need fine-grained control
- The overhead of a recipe isn't justified

## Next Steps

- [Using Recipes](../guides/using-recipes.md) — Apply recipes in your projects
- [Creating Recipes](../guides/creating-recipes.md) — Build custom recipes

[Browse all documentation →](../index.md)
