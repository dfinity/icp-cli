# Motoko Mops Example

This example demonstrates how to build and deploy a Motoko canister that uses the Mops package manager.

## Overview

This project consists of a single Motoko canister that exposes a `greet` function. When called, this function returns a personalized greeting. This example uses the Mops package manager to manage its dependencies.

## Prerequisites

Before you begin, ensure that you have the Motoko compiler (`moc`) and the Mops package manager (`mops`) installed. For installation instructions, please refer to the following resources:

- [Motoko Compiler](https://internetcomputer.org/docs/current/developer-docs/setup/install/)
- [Mops Package Manager](https://mops.one/docs/install)

## Instructions

First, start a local network in a separate terminal window:

```bash
icp network run
```

Then, deploy the canister:

```bash
icp deploy
```

Finally, call the canister:

```bash
icp canister call my-canister greet '("Claude")'
```
