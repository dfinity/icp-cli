# Using Recipes

Recipes are reusable build templates that simplify canister configuration. Instead of writing build steps from scratch, you reference a recipe that expands into the full configuration.

## Why Use Recipes?

- **Less boilerplate** — Common patterns are pre-configured
- **Best practices** — Recipes encode recommended build settings
- **Consistency** — Share build configurations across projects
- **Maintainability** — Update the recipe, update all projects

## Using Official Recipes

DFINITY maintains recipes for common use cases at [github.com/dfinity/icp-cli-recipes](https://github.com/dfinity/icp-cli-recipes).

You can reference recipes by pointing to a URL. For the official recipes, you can use a shorthand for example these recipe types are equivalent:
* `@dfinity/rust`
* `https://github.com/dfinity/icp-cli-recipes/releases/download/rust-latest/recipe.hbs`

### Rust Canister

For building a rust canister:

```yaml
canisters:
  - name: backend
    recipe:
      type: "@dfinity/rust"
      configuration:
        package: backend
```

### Motoko Canister

For building a motoko canister:

```yaml
canisters:
  - name: backend
    recipe:
      type: "@dfinity/motoko"
      configuration:
        main: src/main.mo
```

### Assets Canister

For deploying an asset canister with frontend assets:

```yaml
canisters:
  - name: frontend
    recipe:
      type: "@dfinity/asset-canister"
      configuration:
        dir: dist
```

### Pre-built WASM

For deploying a prebuilt WASM:

```yaml
canisters:
  - name: my-canister
    recipe:
      type: "@dfinity/prebuilt"
      configuration:
        path: ./my-canister.wasm
        sha256: d7c1aba0de1d7152897aeca49bd5fe89a174b076a0ee1cc3b9e45fcf6bde71a6
```

## Recipe Versioning

Pin recipes to specific versions for reproducible builds. These two types are equivalent:

* `@dfinity/rust@v3.0.0`
* `https://github.com/dfinity/icp-cli-recipes/releases/download/rust-v3.0.0/recipe.hbs`

```yaml
canisters:
  - name: backend
    recipe:
      type: "@dfinity/rust@v3.0.0"
      configuration:
        package: backend
```

## Local Recipes

You can create project-specific recipes as Handlebars templates. This can be useful when multiple canisters
in your project share the same build patterns.

```yaml
# recipes/my-rust-canister.hbs
build:
  steps:
    - type: script
      commands:
        - cargo build --package {{package}} --target wasm32-unknown-unknown --release
        - cp target/wasm32-unknown-unknown/release/{{package}}.wasm "$ICP_WASM_OUTPUT_PATH"
```

Reference it in your `icp.yaml`:

```yaml
canisters:
  - name: backend
    recipe:
      type: file://recipes/my-rust-canister.hbs
      configuration:
        package: my-pkg
```

## Remote Recipes

Reference recipes from any URL:

```yaml
canisters:
  - name: backend
    recipe:
      type: https://example.com/recipes/rust-optimized.hbs
      sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
      configuration:
        package: backend
```

Always include `sha256` for remote recipes to ensure integrity.

## Viewing Expanded Configuration

See what a recipe expands to:

```bash
icp project show
```

This displays the effective configuration after all recipes are rendered.

## Recipe Configuration Options

Each recipe defines its own configuration schema. Check the recipe's documentation or source for available options.

## Combining Recipes with Settings

Recipes only define the `build` and `sync` configurations of the canister. You can add canister settings as a separate field
in the configuration file. eg:

```yaml
canisters:
  - name: backend
    recipe:
      type: "@dfinity/rust"
      configuration:
        package: backend
    settings:
      compute_allocation: 10
      environment_variables:
        API_KEY: "secret"
```

## Next Steps

- [Recipes](../concepts/recipes.md) — Understand how recipes work
- [Creating Recipes](creating-recipes.md) — Build custom recipes

[Browse all documentation →](../index.md)
