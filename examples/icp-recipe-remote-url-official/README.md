# ICP Recipe Remote URL Official Example

This example demonstrates how to use a canister recipe that loads template definitions from an official remote URL (GitHub releases).

## Overview

Instead of using built-in recipe types like `motoko` or `rust`, this example shows how to use custom recipe templates hosted on remote servers. This approach allows for:

- Community-contributed recipe templates
- Versioned recipe templates via GitHub releases
- Centralized recipe template management
- Extended functionality beyond built-in types

## Configuration

The [`icp.yaml`](./icp.yaml) file configures a canister that uses a remote recipe template:

```yaml
canister:
  name: my-canister
  recipe:
    type: https://github.com/rikonor/icp-recipes/releases/download/prebuilt-v0.1.2/recipe.hbs
    configuration:
      path: ../icp-pre-built/dist/hello_world.wasm
      sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
```

### Key Components

- **`type`**: Uses an HTTPS URL pointing to a Handlebars template hosted on GitHub releases
- **`configuration`**: Parameters passed to the remote template for customization
- **`path`**: Path to the pre-built WASM file
- **`sha256`**: Checksum for the WASM file to ensure integrity

## How It Works

1. ICP-CLI fetches the recipe template from the remote URL
2. The template is processed using Handlebars with the provided configuration
3. The template generates build and sync steps dynamically
4. The resulting steps are executed to build and deploy the canister

## Use Cases

- Using community-maintained recipe templates
- Sharing recipe templates across multiple projects
- Version-controlled recipe templates via GitHub releases
- Templates with complex build logic that aren't built into ICP-CLI

## Related Examples

- [`icp-recipe-local-file`](../icp-recipe-local-file/): Using local file recipe templates
- [`icp-recipe-remote-url`](../icp-recipe-remote-url/): Using remote URL recipes (custom server)
- [`icp-pre-built`](../icp-pre-built/): Using the built-in pre-built recipe type
