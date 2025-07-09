# Rust Canister Example

This example demonstrates how to build and deploy a Rust canister using `icp`.

## Overview

This project consists of a single Rust canister that exposes a `greet` function. When called, this function returns a personalized greeting.

## Prerequisites

Before you begin, ensure that you have the `wasm32-unknown-unknown` Rust toolchain installed:

```bash
rustup target add wasm32-unknown-unknown
```

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
