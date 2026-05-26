---
title: Creating Recipes
description: Author custom Handlebars-based recipe templates to encode build conventions and share them across projects.
---

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
        - echo "Building {{_.canister.name}}..."

{{! # optional sync step }}
sync:
  steps:
    - type: script
      commands:
        - echo "Syncing {{_.canister.name}}..."
```

## Basic Recipe Example

A simple recipe for Rust builds using `{{_.canister.name}}`:

```
{{! file: ./recipes/rust-example.hbs }}
{{! A recipe for building a rust canister }}
{{! `shrink: boolean` Optimizes the wasm with ic-wasm }}

build:
  steps:
    - type: script
      commands:
        - cargo build --package {{_.canister.name}} --target wasm32-unknown-unknown --release
        - mv target/wasm32-unknown-unknown/release/{{ replace "-" "_" _.canister.name }}.wasm "$ICP_WASM_OUTPUT_PATH"

    - type: script
      commands:
        - command -v ic-wasm >/dev/null 2>&1 || { echo >&2 'ic-wasm not found. To install ic-wasm, see https://github.com/dfinity/ic-wasm \n'; exit 1; }
        - ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "${ICP_WASM_OUTPUT_PATH}" metadata "cargo:version" -d "$(cargo --version)" --keep-name-section
        - ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "${ICP_WASM_OUTPUT_PATH}" metadata "template:type" -d "rust" --keep-name-section
        {{#if shrink}}
        - ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "${ICP_WASM_OUTPUT_PATH}" shrink --keep-name-section
        {{/if}}
```

Usage — no `package` field needed since `{{_.canister.name}}` is injected automatically:

```yaml
# file: icp.yaml
canisters:
  - name: backend
    recipe:
      type: ./recipes/rust-example.hbs
      configuration:
        shrink: true
```

## Template Syntax

Recipes use [Handlebars](https://handlebarsjs.com/) templating:

### Variables

Access configuration parameters passed in the `configuration` section of the recipe, and built-in `_.*` variables provided automatically by icp-cli:

```
build:
  steps:
    - type: script
      commands:
        - cargo build --package {{_.canister.name}} --target wasm32-unknown-unknown --release
        - cp "target/wasm32-unknown-unknown/release/{{ replace "-" "_" _.canister.name }}.wasm" "$ICP_WASM_OUTPUT_PATH"
        {{#if shrink}}
        - ic-wasm "$ICP_WASM_OUTPUT_PATH" -o "$ICP_WASM_OUTPUT_PATH" shrink
        {{/if}}
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
        - ic-wasm target/wasm32-unknown-unknown/release/{{ replace "-" "_" _.canister.name }}.wasm -o "$ICP_WASM_OUTPUT_PATH" shrink
        {{else}}
        - cargo build --target wasm32-unknown-unknown
        - cp target/wasm32-unknown-unknown/debug/{{ replace "-" "_" _.canister.name }}.wasm "$ICP_WASM_OUTPUT_PATH"
        {{/if}}
```

### Loops

Use `{{#each}}` for dynamic lists:

```
{{! file: ./recipes/rust-example-metadata.hbs }}
{{! A recipe for building a rust canister }}
{{! `metadata: [name: string, value: string]`: An array of name/value pairs that get injected into the wasm metadata section }}

build:
  steps:
    - type: script
      commands:
        - cargo build --package {{_.canister.name}} --target wasm32-unknown-unknown --release
        - mv target/wasm32-unknown-unknown/release/{{ replace "-" "_" _.canister.name }}.wasm "$ICP_WASM_OUTPUT_PATH"

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
        metadata:
          - name: "crate:version"
            value: "1.0.0"
          - name: "build:profile"
            value: "release"
```

### Default Values

Use `{{#if}}` with `{{else}}` for defaults, refer to the examples above.

## Built-in Recipe Variables

icp-cli automatically injects variables into every recipe template under the reserved `icp` namespace. These are available alongside any user-provided `configuration:` values and cannot be overridden by them.

| Variable | Value |
|---|---|
| `{{_.canister.name}}` | The canister name as defined in `icp.yaml` |

Use `{{_.canister.name}}` whenever a recipe needs to refer to the canister being built — this avoids requiring users to repeat the name in the `configuration:` block.

Built-in recipe variables work with all Handlebars helpers. For example, the `replace` helper can produce the underscore form of a name required by Rust WASM artifact filenames:

```
- cargo build --package {{_.canister.name}} --target wasm32-unknown-unknown --release
- cp "target/wasm32-unknown-unknown/release/{{ replace "-" "_" _.canister.name }}.wasm" "$ICP_WASM_OUTPUT_PATH"
```

User-provided overrides can still be supported with an `{{#if}}` fallback for cases where the user needs to supply a different name (e.g. when the Cargo package name differs from the canister name):

```
{{#if package}}{{package}}{{else}}{{_.canister.name}}{{/if}}
```

### Built-in recipe variables vs. environment variables

icp-cli provides two distinct kinds of variables to recipes:

- **`{{_.*}}` built-in recipe variables** — injected at _render time_, when the recipe template is expanded into build/sync steps. Use these in Handlebars expressions.
- **`$ICP_*` environment variables** — set at _execution time_, when the rendered build commands actually run. Use these inside shell commands.

`{{_.canister.name}}` is available at render time because it is read from `icp.yaml` before any build command is run. `$ICP_WASM_OUTPUT_PATH` must be an environment variable because it is a temporary path computed dynamically at execution time.

## Environment Variables

Recipe scripts have access to runtime environment variables set by icp-cli.

**Build script steps** receive:

- `ICP_WASM_OUTPUT_PATH` — Where to write the compiled WASM file

**Sync script steps** receive:

- `ICP_CLI_ENVIRONMENT` — The current environment name (e.g. `local`, `staging`)
- `ICP_CLI_NETWORK` — The current network name (e.g. `local`, `ic`)
- `ICP_CLI_CID` — The canister ID of the canister being synced
- `ICP_CLI_CID_<NAME>` — The canister ID of every canister with a registered ID in the current environment

See [Environment Variables Reference](../reference/environment-variables.md) for full details.

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
  - name: canister2
    recipe:
      type: ./recipes/my-recipe.hbs
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
