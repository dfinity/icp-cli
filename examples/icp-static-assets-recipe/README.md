# ICP Static Assets Recipe Example

This example demonstrates how to use the built-in `assets` recipe type to deploy static assets to an assets canister.

## Overview

The `assets` recipe type provides a streamlined way to deploy static websites and assets to the Internet Computer using the official assets canister. This is one of the built-in recipe types provided by ICP-CLI for hosting static content.

## Configuration

The [`icp.yaml`](./icp.yaml) file configures a canister using the `@dfinity/asset-canister` recipe:

```yaml
canisters:
  - name: my-canister
    recipe:
      type: "@dfinity/asset-canister"
      configuration:
        version: 0.29.2
        dir: www
```

### Key Components

- **`type: "@dfinity/asset-canister"`**: Uses the official DFINITY asset canister recipe
- **`version`**: Specifies the assets canister version to use (optional, defaults to latest)
- **`dir`**: Directory containing static assets to deploy (required)

## Assets Directory

The [`www/`](./www/) directory contains the static files that will be deployed:

- [`index.html`](./www/index.html): Main HTML file served by the assets canister

## How It Works

1. ICP-CLI uses the built-in `assets` recipe resolver
2. The resolver generates build steps that:
   - Download the official assets canister WASM from the specified version
   - Configure the canister for asset serving
3. Sync steps are generated to:
   - Upload all files from the specified directory to the assets canister
   - Set appropriate content types and caching headers

## Use Cases

- Static website hosting
- Single Page Applications (SPAs)
- Asset hosting for frontend applications
- Simple web content deployment

## Related Examples

- [`icp-static-assets`](../icp-static-assets/): Static assets with explicit build/sync configuration
- [`icp-static-react-site`](../icp-static-react-site/): React application deployment
