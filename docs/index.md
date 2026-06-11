---
title: icp-cli Documentation
description: Build and deploy applications on the Internet Computer using icp-cli, with links to quickstart, tutorials, guides, and reference documentation.
---

Build and deploy applications on the [Internet Computer](https://internetcomputer.org).

## Start Here

- **[Quickstart](quickstart.md)** — Deploy a fullstack app in under 5 minutes
- **[Tutorial](tutorial.md)** — Deploy your first app step by step

## Guides

Step-by-step instructions for common tasks:

- [Installation](guides/installation.md) — Install icp-cli on your system
- [Local Development](guides/local-development.md) — The edit-build-deploy cycle
- [Deploying to Mainnet](guides/deploying-to-mainnet.md) — Go live on the Internet Computer
- [Deploying to Specific Subnets](guides/deploying-to-specific-subnets.md) — Target specific subnets
- [Canister Snapshots](guides/canister-snapshots.md) — Back up and restore canister state
- [Canister Migration](guides/canister-migration.md) — Move canisters between subnets
- [Managing Environments](guides/managing-environments.md) — Dev, staging, production workflows
- [Managing Identities](guides/managing-identities.md) — Keys and authentication reference
- [Tokens and Cycles](guides/tokens-and-cycles.md) — ICP tokens and cycles command reference
- [Proxy Canister](guides/proxy-canister.md) — Forward calls with cycles or call canister-only methods
- [Containerized Networks](guides/containerized-networks.md) — Docker-based local networks
- [Using Recipes](guides/using-recipes.md) — Reusable build templates
- [Creating Recipes](guides/creating-recipes.md) — Build custom recipes
- [Creating Templates](guides/creating-templates.md) — Author project templates
- [Writing a Sync Plugin](guides/writing-sync-plugins.md) — Author a sandboxed WebAssembly sync plugin

## Concepts

Understand how icp-cli works:

- [Project Model](concepts/project-model.md) — How configuration is organized
- [Build, Deploy, Sync](concepts/build-deploy-sync.md) — The deployment lifecycle
- [Environments and Networks](concepts/environments.md) — Deployment targets explained
- [Recipes](concepts/recipes.md) — Templated build configurations
- [Sync Plugins](concepts/sync-plugins.md) — Sandboxed WebAssembly components for the sync phase
- [Canister Discovery](concepts/canister-discovery.md) — How canisters discover each other
- [Binding Generation](concepts/binding-generation.md) — Type-safe canister interfaces

## Reference

Complete technical specifications:

- [CLI Reference](reference/cli.md) — All commands and flags
- [Configuration Reference](reference/configuration.md) — icp.yaml schema
- [Canister Settings](reference/canister-settings.md) — All settings options
- [Environment Variables](reference/environment-variables.md) — CLI and build variables

## Additional Resources

- [Migrating from dfx](migration/from-dfx.md) — For existing dfx users
- [Upgrading from icp-cli 0.2](migration/upgrading-from-v0-2.md) — Switch off the removed `type: assets` sync step
- [Telemetry](telemetry.md) — What data is collected and how to opt out
- [Examples](https://github.com/dfinity/icp-cli/tree/main/examples) — Sample projects for various use cases
