# Concepts

Understanding how icp-cli organizes and manages your project.

## Core Concepts

- [Project Model](project-model.md) — How icp-cli discovers and consolidates configuration
- [Build, Deploy, Sync](build-deploy-sync.md) — The three phases of the deployment lifecycle
- [Environments and Networks](environments.md) — Deployment targets and how they relate
- [Recipes](recipes.md) — Templated, reusable build configurations
- [Sync Plugins](sync-plugins.md) — Sandboxed WebAssembly components that run during the sync phase
- [Canister Discovery](canister-discovery.md) — How canisters discover each other
- [Project Dependencies](project-dependencies.md) — Depending on other vendored icp projects and deploying them as a workspace
- [Binding Generation](binding-generation.md) — Type-safe canister interfaces

## Quick Reference

| Term | Definition |
|------|------------|
| **Project** | A directory containing `icp.yaml` and your canister source code |
| **Canister** | A unit of deployment on the Internet Computer — your compiled WASM plus settings |
| **Network** | An ICP network endpoint — local (managed by icp-cli) or remote (mainnet, testnet) |
| **Environment** | A named deployment target combining a network with canister settings |
| **Recipe** | A Handlebars template that generates build and sync configuration |
| **Sync plugin** | A sandboxed WebAssembly component that runs post-deployment logic during `icp sync` |
| **Principal** | A public identifier for an identity or canister on the Internet Computer |
