---
title: Managing Environments
description: Configure and use dev, staging, and production environments to deploy independent canister instances with different settings.
---

Environments allow you to deploy multiple instances of a set of canisters to the same network, with each set having independent settings. This guide covers setting up development, staging, and production environments.

## Understanding Environments

An **environment** combines:
- A **network** to deploy to
- A set of **canisters** to deploy
- **Settings** specific to that environment

Two implicit environments are always available:
- `local` — Uses the local managed network (default)
- `ic` — Uses the IC mainnet

## Basic Environment Configuration

Add environments to your `icp.yaml`:

```yaml
canisters:
  - name: frontend
    build:
      # ... build steps
  - name: backend
    build:
      # ... build steps

environments:
  - name: staging
    network: ic
    canisters: [frontend, backend]

  - name: production
    network: ic
    canisters: [frontend, backend]
```

## Excluding Canisters

Instead of listing everything an environment deploys, you can list what it leaves out. `exclude-canisters:` works with or without `canisters:` — with no `canisters:` it starts from the whole project, and alongside one it narrows that selection further:

```yaml
environments:
  # Everything except the mock ledger, which only local development needs.
  - name: production
    network: ic
    exclude-canisters: [mock-ledger]

  # An explicit selection, minus one of its subproject's canisters.
  - name: staging
    network: ic
    canisters: [frontend, backend, "services/crm:worker"]
    exclude-canisters: ["services/crm:worker"]
```

An entry names a canister the same way the rest of the project does: a bare local name such as `backend`, or a `subproject:canister` key such as `services/crm:worker`. To leave out a whole subproject — its own canisters and those of any subprojects nested beneath it — name it with a trailing `:` and no canister name:

```yaml
environments:
  - name: production
    network: ic
    exclude-canisters:
      - services/crm:
```

Because such an entry ends in `:`, YAML needs it quoted inside a bracketed list: `exclude-canisters: ["services/crm:"]`.

An entry that matches no canister in the project is an error, so a misspelled name fails at load time.

A subproject's own environment may exclude canisters too, and its entries hold in the workspace exactly as they do when it is deployed on its own. It writes them relative to itself — a bare name for one of its own canisters, `vendor/ledger:` for a subproject it vendors — and may only reach inside its own subtree, so it can never exclude a canister belonging to the project that vendored it. Exclusions never conflict: the root's and every subproject's simply add up.

## Environment-Specific Settings

Override canister settings per environment:

```yaml
environments:
  - name: staging
    network: ic
    canisters: [frontend, backend]
    settings:
      backend:
        compute_allocation: 5
        environment_variables:
          LOG_LEVEL: "debug"

  - name: production
    network: ic
    canisters: [frontend, backend]
    settings:
      backend:
        compute_allocation: 20
        freezing_threshold: 90d
        environment_variables:
          LOG_LEVEL: "error"
```

## Deploying to Environments

Deploy to a specific environment:

```bash
# Local development (default)
icp deploy

# Staging
icp deploy --environment staging

# Production
icp deploy --environment production

# IC mainnet (using implicit ic environment)
icp deploy -e ic
```

## Environment-Specific Init Args

Provide different initialization arguments per environment:

```yaml
canisters:
  - name: backend
    build:
      # ... build steps
    init_args: "(record { mode = \"production\" })"

environments:
  - name: staging
    network: ic
    canisters: [backend]
    init_args:
      backend: "(record { mode = \"staging\" })"
```

## Viewing Environment Configuration

See all configured environments:

```bash
icp environment list
```

View the effective project configuration:

```bash
icp project show
```

This shows all environments and their settings.

## Working with Canister IDs

Each environment maintains separate canister IDs. The storage location depends on network type:

- **Managed networks** (local): `.icp/cache/mappings/<environment>.ids.json`
- **Connected networks** (IC mainnet): `.icp/data/mappings/<environment>.ids.json`

List canisters configured for an environment:

```bash
icp canister list --environment staging
```

This shows the network status of the canisters in that environment:

```bash
icp canister status --environment staging
```

## Example: Full Multi-Environment Setup

```yaml
canisters:
  - name: frontend
    build:
      steps:
        - type: script
          commands:
            - npm run build
    sync:
      steps:
        - type: plugin
          path: ./plugins/upload-assets.wasm
          dirs:
            - dist

  - name: backend
    build:
      steps:
        - type: script
          commands:
            - cargo build --target wasm32-unknown-unknown --release
            - cp target/wasm32-unknown-unknown/release/backend.wasm "$ICP_WASM_OUTPUT_PATH"

environments:
  - name: staging
    network: ic
    canisters: [frontend, backend]
    settings:
      frontend:
        memory_allocation: 2gib
      backend:
        compute_allocation: 5
        reserved_cycles_limit: 5t
        environment_variables:
          API_ENV: "staging"

  - name: production
    network: ic
    canisters: [frontend, backend]
    settings:
      frontend:
        memory_allocation: 4gib
        freezing_threshold: 90d
      backend:
        compute_allocation: 20
        reserved_cycles_limit: 50t
        freezing_threshold: 90d
        environment_variables:
          API_ENV: "production"
```

## Deployment Workflow

A typical workflow:

```bash
# 1. Develop locally
icp network start -d
icp build && icp deploy
# ... test changes ...

# 2. Deploy to staging
icp deploy --environment staging
# ... verify on staging ...

# 3. Deploy to production
icp deploy --environment production
```

## Next Steps

- [Environments and Networks](../concepts/environments.md) — Understand how environments work

[Browse all documentation →](../index.md)
