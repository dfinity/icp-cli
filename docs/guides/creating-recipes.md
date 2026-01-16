# Creating Recipes

Recipes are reusable build templates that you can create to encode your team's build conventions or share them with the community.

## Recipe File Structure

A recipe is a [handlebars](https://handlebarsjs.com) template that renders to yaml and contains the `build` and `sync` steps
of a canister configuration.

```
{{! # recipes/my-recipe.hbs }}
build:
  steps:
    - type: script
      commands:
        - echo "Building {{ name }}..."

{{! # optional sync step }}
sync:
  steps:
    - type: script
      commands:
        - echo "Syncing {{ name }}..."
```

## Basic Recipe Example

A simple recipe for Rust builds:

```
{{! file: ./recipes/rust-example.hbs }}
{{! A recipe for building a rust canister }}
{{! `package: string` The package to build }}
{{! `shrink: boolean` Optimizes the wasm with ic-wasm }}

build:
  steps:
    - type: script
      commands:
        - cargo build --package {{ package }} --target wasm32-unknown-unknown --release
        - mv target/wasm32-unknown-unknown/release/{{ replace "-" "_" package }}.wasm "$ICP_WASM_OUTPUT_PATH"

    - type: script
      commands:
        - command -v ic-wasm >/dev/null 2>&1 || { echo >&2 'ic-wasm not found. To install ic-wasm, see https://github.com/dfinity/ic-wasm \n'; exit 1; }
        - ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "${ICP_WASM_OUTPUT_PATH}" metadata "cargo:version" -d "$(cargo --version)" --keep-name-section
        - ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "${ICP_WASM_OUTPUT_PATH}" metadata "template:type" -d "rust" --keep-name-section
        {{#if shrink}}
        - ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "${ICP_WASM_OUTPUT_PATH}" shrink --keep-name-section
        {{/if}}
```

Usage:

```yaml
# file: icp.yaml
canisters:
  - name: backend
    recipe:
      type: ./recipes/rust-example.hbs
      configuration:
        package: my-backend-crate
        shrink: true
```

## Template Syntax

Recipes use [Handlebars](https://handlebarsjs.com/) templating:

### Variables

Access configuration parameters passed in the `configuration` section of the recipe.

```
build:
  steps:
    - type: script
      commands:
        - cargo build --package {{configuration.package}}
```

### Conditionals

Use `{{#if}}` for optional configuration:

```
build:
  steps:
    - type: script
      commands:
        {{#if shrink}}
        - cargo build --release --target wasm32-unknown-unknown
        - ic-wasm target/wasm32-unknown-unknown/release/{{configuration.package}}.wasm -o "$ICP_WASM_OUTPUT_PATH" shrink
        {{else}}
        - cargo build --target wasm32-unknown-unknown
        - cp target/wasm32-unknown-unknown/debug/{{configuration.package}}.wasm "$ICP_WASM_OUTPUT_PATH"
        {{/if}}
```

### Loops

Use `{{#each}}` for dynamic lists:

```
{{! file: ./recipes/rust-example-metadata.hbs }}
{{! A recipe for building a rust canister }}
{{! `package: string` The package to build }}
{{! `metadata: [name: string, value: string]`: An array of name/value pairs that get injected into the wasm metadata section }}

build:
  steps:
    - type: script
      commands:
        - cargo build --package {{ package }} --target wasm32-unknown-unknown --release
        - mv target/wasm32-unknown-unknown/release/{{ replace "-" "_" package }}.wasm "$ICP_WASM_OUTPUT_PATH"

    - type: script
      commands:
        - command -v ic-wasm >/dev/null 2>&1 || { echo >&2 'ic-wasm not found. To install ic-wasm, see https://github.com/dfinity/ic-wasm \n'; exit 1; }
        {{#if metadata}}
        {{#each metadata}}
        - ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "${ICP_WASM_OUTPUT_PATH}" metadata "{{ name }}" -d "{{ value }}" --keep-name-section
        {{/each}}
        {{/if}}
```

```yaml
# file: icp.yaml
canisters:
  - name: backend
    recipe:
      type: ./recipes/rust-example-metadata.hbs
      configuration:
        package: my-backend-crate
        metadata:
          - name: "crate:version"
            value: "1.0.0"
          - name: "build:profile"
            value: "release"
```

### Default Values

Use `{{#if}}` with `{{else}}` for defaults, refer to the examples above.

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

### Within a project

Store recipes in your project's `recipes/` directory and reference with relative paths:

```yaml
# file: icp.yaml
canisters:
  - name: canister1
    recipe:
      type: ./recipes/my-recipe.hbs
      configuration:
        package: my-crate1
  - name: canister2
    recipe:
      type: ./recipes/my-recipe.hbs
      configuration:
        package: my-other-crate
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

For examples of recipes, you can check out [github.com/dfinity/icp-cli-recipes](https://github.com/dfinity/icp-cli-recipes).


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
