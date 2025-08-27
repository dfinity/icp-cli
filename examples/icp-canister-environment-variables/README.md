# Canister Environment Variables Example

This example demonstrates how to configure environment variables for an ICP canister using the `icp.yaml` configuration file.

## Overview

Environment variables allow you to pass configuration values to your canister at deployment time. These variables are set as part of the canister's settings and can be accessed within your canister code.

## Configuration

The [`icp.yaml`](icp.yaml) file shows how to define environment variables in the canister settings:

```yaml
canister:
  name: my-canister

  build:
    steps:
      - type: pre-built
        path: ../icp-pre-built/dist/hello_world.wasm
        sha256: 17a05e36278cd04c7ae6d3d3226c136267b9df7525a0657521405e22ec96be7a

  settings:
    environment_variables:
      var-1: value-1
      var-2: value-2
      var-3: value-3
```

## Key Features

- **Environment Variables**: Define key-value pairs that will be available to your canister
- **Flexible Configuration**: Set different values for different environments
- **Runtime Access**: Environment variables can be accessed within your canister code

## Usage

1. Define your environment variables in the `settings.environment_variables` section
2. Deploy the canister using `icp-cli deploy`
3. The environment variables will be automatically configured during canister installation

## Environment Variable Access

The specific method to access environment variables depends on your canister's programming language:

- **Motoko**: Use the `Debug.print` or system APIs to access environment configuration
- **Rust**: Use the IC CDK functions to read canister settings
- **Other languages**: Refer to the respective IC SDK documentation

## Related Examples

- [`icp-canister-settings`](../icp-canister-settings/): Shows other canister settings options
- [`icp-pre-built`](../icp-pre-built/): Demonstrates using pre-built WASM files

## Learn More

- [Internet Computer Documentation](https://internetcomputer.org/docs)
- [Canister Settings Reference](https://internetcomputer.org/docs/current/references/ic-interface-spec#ic-create_canister)
