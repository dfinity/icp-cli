# ICP Recipe Remote URL Example

This example demonstrates how to use a canister recipe that loads template definitions from a remote URL served by a local development server.

## Overview

This example shows how to use custom recipe templates hosted on a local HTTP server during development. This approach is useful for:

- Testing remote recipe template functionality locally
- Developing and debugging recipe templates
- Simulating production remote recipe workflows
- Custom recipe template servers

## Configuration

The [`icp.yaml`](./icp.yaml) file configures a canister that uses a remote recipe template from localhost:

```yaml
canister:
  name: my-canister
  recipe:
    type: http://localhost:8080/recipe.hb.yaml
    configuration:
      path: ../icp-pre-built/dist/hello_world.wasm
      sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
```

### Key Components

- **`type`**: Uses an HTTP URL pointing to a local development server
- **`configuration`**: Parameters passed to the remote template for customization
- **`path`**: Path to the pre-built WASM file
- **`sha256`**: Checksum for the WASM file to ensure integrity

## Development Server

This example includes a [`Makefile`](./Makefile) that provides commands to start a local HTTP server:

```bash
make serve
```

The server hosts the [`recipe.hb.yaml`](./recipe.hb.yaml) template file, making it accessible via HTTP for testing remote recipe functionality.

## How It Works

1. Start the local HTTP server using `make serve`
2. ICP-CLI fetches the recipe template from `http://localhost:8080/recipe.hb.yaml`
3. The template is processed using Handlebars with the provided configuration
4. The template generates build and sync steps dynamically
5. The resulting steps are executed to build and deploy the canister

## Use Cases

- Testing remote recipe template functionality during development
- Prototyping custom recipe template servers
- Debugging template logic before deployment
- Development workflow for custom recipe templates

## Related Examples

- [`icp-recipe-remote-url-official`](../icp-recipe-remote-url-official/): Using remote GitHub-hosted recipe templates
- [`icp-recipe-local-file`](../icp-recipe-local-file/): Using local file recipe templates
- [`icp-pre-built`](../icp-pre-built/): Using the built-in pre-built recipe type
