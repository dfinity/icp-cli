# Motoko Example

This example demonstrates how to build and deploy a simple Motoko canister using `icp`.

## Overview

This project consists of a single Motoko canister that exposes a `greet` function. When called, this function returns a personalized greeting.

## Prerequisites

Before you begin, ensure that you have the Motoko compiler (`moc`) installed. For installation instructions, please refer to the [official documentation](https://internetcomputer.org/docs/current/developer-docs/setup/install/).

## Instructions

First, start a local network in a separate terminal window:

```bash
icp network start
```

Then, deploy the canister:

```bash
icp deploy
```

Finally, call the canister:

```bash
icp canister call my-canister greet '("Claude")'
```
