# Managing Environments

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
        freezing_threshold: 7776000  # 90 days
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
        - type: assets
          dir: dist

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
        memory_allocation: 2147483648  # 2GB
      backend:
        compute_allocation: 5
        reserved_cycles_limit: 5000000000000
        environment_variables:
          API_ENV: "staging"

  - name: production
    network: ic
    canisters: [frontend, backend]
    settings:
      frontend:
        memory_allocation: 4294967296  # 4GB
        freezing_threshold: 7776000    # 90 days
      backend:
        compute_allocation: 20
        reserved_cycles_limit: 50000000000000
        freezing_threshold: 7776000
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
