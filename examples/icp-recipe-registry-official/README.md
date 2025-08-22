# ICP Recipe Registry Official Example

This example demonstrates how to use canister recipes from the official ICP recipe registry using the `@` syntax, similar to npm package references.

## Overview

The registry-style recipe syntax allows you to reference well-maintained, versioned recipe templates from official repositories. This approach provides:

- Centralized recipe management and distribution
- Semantic versioning for recipe templates
- Official recipes maintained by the ICP team
- Simplified recipe referencing without full URLs

## Configuration

The [`icp.yaml`](./icp.yaml) file demonstrates three different ways to reference registry recipes:

```yaml
canisters:
  - name: my-canister
    recipe:
      type: "@dfinity/prebuilt"
      configuration:
        path: ../icp-pre-built/dist/hello_world.wasm
        sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a

  - name: my-canister-with-latest
    recipe:
      type: "@dfinity/prebuilt@latest"
      configuration:
        path: ../icp-pre-built/dist/hello_world.wasm
        sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a

  - name: my-canister-with-version
    recipe:
      type: "@dfinity/prebuilt@v1.0.5"
      configuration:
        path: ../icp-pre-built/dist/hello_world.wasm
        sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a
```

### Registry Syntax Variants

1. **Default Version**: `@dfinity/prebuilt`
   - Uses the default version (typically latest stable)
   - Simplest form for getting started

2. **Latest Version**: `@dfinity/prebuilt@latest`
   - Explicitly requests the latest available version
   - Useful for getting cutting-edge features

3. **Specific Version**: `@dfinity/prebuilt@v1.0.5`
   - Locks to a specific version for reproducible builds
   - Recommended for production deployments

## How It Works

1. ICP-CLI resolves the registry reference (`@dfinity/prebuilt`) to the official recipe repository
2. The specified version (or default) is fetched from the registry
3. The recipe template is downloaded and processed with the provided configuration
4. Build and sync steps are generated based on the template
5. The canister is built and deployed according to the resolved instructions

## Benefits

- **Version Control**: Pin to specific recipe versions for consistent builds
- **Official Support**: Use recipes maintained by the ICP team
- **Easy Updates**: Upgrade to newer recipe versions by changing the version tag
- **Discoverability**: Browse available recipes in the official registry
- **Reliability**: Recipes are tested and validated before publishing

## Registry vs Other Recipe Types

| Recipe Type | Use Case | Example |
|-------------|----------|---------|
| Registry (`@scope/name`) | Official, versioned recipes | `@dfinity/prebuilt@v1.0.5` |
| Remote URL | Custom/community recipes | `https://github.com/user/repo/recipe.hbs` |
| Local File | Development/testing | `file://recipe.hb.yaml` |
| Built-in | Simple, common patterns | `motoko`, `rust`, `assets` |

## Related Examples

- [`icp-recipe-remote-url-official`](../icp-recipe-remote-url-official/): Using GitHub-hosted recipe templates
- [`icp-recipe-local-file`](../icp-recipe-local-file/): Using local file recipe templates
- [`icp-pre-built`](../icp-pre-built/): Using the built-in pre-built recipe type
