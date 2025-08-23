# ICP Environments Example

This example demonstrates how to configure deployment environments with custom networks and canister settings.

## Overview

Environments allow you to define different deployment targets with specific configurations for networks, canisters, and settings. This is useful for managing deployments across development, staging, and production environments.

## Configuration

The [`icp.yaml`](./icp.yaml) file configures:

1. A canister with pre-built WASM
2. A custom network configuration
3. An environment that ties them together

```yaml
canister:
  name: my-canister
  build:
    steps:
      - type: pre-built
        path: ../icp-pre-built/dist/hello_world.wasm
        sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a

networks:
  - name: my-network
    mode: managed
    gateway:
      host: 127.0.0.1

environments:
  - name: my-environment
    # deployment target
    network: my-network
    # canisters to deploy
    canisters: [my-canister]
    # override canister settings
    settings:
      my-canister:
        memory_allocation: 10
```

### Key Components

- **`canister`**: Defines the canister to be deployed using a pre-built WASM
- **`networks`**: Configures custom network settings (managed local network)
- **`environments`**: Links canisters to networks with specific deployment settings

## Environment Features

The environment configuration provides:

- **Network Targeting**: Deploy to specific networks (local, testnet, mainnet)
- **Canister Selection**: Choose which canisters to deploy in each environment
- **Settings Override**: Customize canister settings per environment (memory allocation, compute allocation, etc.)
- **Deployment Isolation**: Separate configurations for different deployment stages

## Use Cases

- **Development**: Local network with debug settings
- **Staging**: Testnet deployment with production-like settings
- **Production**: Mainnet deployment with optimized settings
- **Testing**: Isolated environments for automated testing

## Deployment

Deploy to the configured environment:

```bash
icp deploy --environment my-environment
```

## Related Examples

- [`icp-network-connected`](../icp-network-connected/): External network configuration
- [`icp-network-inline`](../icp-network-inline/): Inline network definitions
- [`icp-canister-settings`](../icp-canister-settings/): Canister settings configuration
