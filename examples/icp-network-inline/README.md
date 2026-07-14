# Inline Network Example

This example demonstrates how to define an inline network in an ICP project manifest.

## Overview

This project defines a `staging` network that is an alias for the mainnet. This allows you to deploy a separate set of canisters to the mainnet for staging purposes.

## Instructions

This project also defines a `staging` environment that targets the `staging` network. To deploy the canister to it, run:

```bash
icp deploy --environment staging
```
