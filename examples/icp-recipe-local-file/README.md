# ICP Recipe Local File Example

This example demonstrates how to use a canister recipe that loads template definitions from a local file.

## Overview

Instead of using built-in recipe types like `motoko` or `rust`, this example shows how to use custom recipe templates stored locally. This approach allows for:

- Custom build logic defined in local templates
- Easy testing and development of recipe templates
- Project-specific build workflows
- Template reuse across local canisters

## Configuration

The [`icp.yaml`](./icp.yaml) file configures a canister that uses a local file recipe template:

```yaml
canister:
  name: my-canister
  recipe:
    type: file://recipe.hb.yaml
    configuration:
      path: ../icp-pre-built/dist/hello_world.wasm
      sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
```

### Key Components

- **`type`**: Uses the `file://` prefix to specify a local file path
- **`configuration`**: Parameters passed to the local template for customization
- **`path`**: Path to the pre-built WASM file
- **`sha256`**: Checksum for the WASM file to ensure integrity

## Template File

The [`recipe.hb.yaml`](./recipe.hb.yaml) file contains the Handlebars template that defines the build and sync steps. This template is processed with the configuration parameters to generate the actual build instructions.

## How It Works

1. ICP-CLI reads the recipe template from the local file
2. The template is processed using Handlebars with the provided configuration
3. The template generates build and sync steps dynamically
4. The resulting steps are executed to build and deploy the canister

## Use Cases

- Developing custom recipe templates
- Project-specific build workflows
- Testing template logic before publishing remotely
- Complex build processes that require custom logic

## Related Examples

- [`icp-recipe-remote-url-official`](../icp-recipe-remote-url-official/): Using remote GitHub-hosted recipe templates
- [`icp-recipe-remote-url`](../icp-recipe-remote-url/): Using remote URL recipes (custom server)
- [`icp-pre-built`](../icp-pre-built/): Using the built-in pre-built recipe type
