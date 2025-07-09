# Multi-canister Project Example

This example demonstrates how to configure and deploy a multi-canister project using `icp`.

## Overview

This project consists of two canisters, `canister-1` and `canister-2`, which are both pre-built. The `icp.yaml` file at the root of the project references the individual canister configuration files located in the `canisters` directory.

## Instructions

First, start a local network in a separate terminal window:

```bash
icp network run
```

Then, deploy the canisters:

```bash
icp deploy
```
