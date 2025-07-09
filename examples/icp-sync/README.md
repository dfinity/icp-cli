# Sync Example

This example demonstrates how to use the `sync` command to interact with a canister after it has been deployed.

## Overview

This project consists of a single Motoko canister that stores a natural number. The `icp.yaml` file is configured to call the `set` method of the canister during the `sync` step, which initializes the number to `1`.

## Prerequisites

Before you begin, ensure that you have the Motoko compiler (`moc`) installed. For installation instructions, please refer to the [official documentation](https://internetcomputer.org/docs/current/developer-docs/setup/install/).

## Instructions

First, start a local network in a separate terminal window:

```bash
icp network run
```

Then, deploy the canister and run the sync command:

```bash
icp deploy
```

Finally, you can call the `get` method to verify that the number has been set to `1`:

```bash
icp canister call my-canister get
```
