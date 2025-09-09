# Advanced Workflows

This guide covers advanced usage patterns and workflows for ICP CLI, including multi-environment deployments, CI/CD integration, and complex project management scenarios.

## Multi-Environment Deployments

### Environment Strategy

Set up multiple environments for different stages of your development lifecycle:

```yaml
# icp.yaml
canisters:
  - frontend/canister.yaml
  - backend/canister.yaml
  - database/canister.yaml

networks:
  - name: local
    mode: managed
    gateway:
      host: 127.0.0.1
      port: 4943

  - name: testnet
    mode: external  
    gateway:
      url: https://testnet.ic0.app

  - name: staging
    mode: external
    gateway:
      url: https://ic0.app

  - name: production
    mode: external
    gateway:
      url: https://ic0.app

environments:
  # Development environment - local testing
  - name: dev
    network: local
    canisters: [frontend, backend, database]
    settings:
      frontend:
        memory_allocation: 1073741824  # 1GB
        environment_variables:
          NODE_ENV: "development"
          API_BASE_URL: "http://localhost:4943"
          DEBUG_MODE: "true"
      backend:
        compute_allocation: 1
        environment_variables:
          LOG_LEVEL: "debug"
          CACHE_TTL: "60"

  # Testing environment - automated testing  
  - name: test
    network: testnet
    canisters: [frontend, backend, database]
    settings:
      frontend:
        memory_allocation: 2147483648  # 2GB
        environment_variables:
          NODE_ENV: "test"
          API_BASE_URL: "https://testnet.ic0.app"
      backend:
        compute_allocation: 2
        reserved_cycles_limit: 5000000000000
        environment_variables:
          LOG_LEVEL: "info"

  # Staging environment - pre-production testing
  - name: staging
    network: staging
    canisters: [frontend, backend, database]
    settings:
      frontend:
        memory_allocation: 4294967296  # 4GB
        compute_allocation: 5
        freezing_threshold: 2592000    # 30 days
        environment_variables:
          NODE_ENV: "staging"
          API_BASE_URL: "https://ic0.app"
      backend:
        compute_allocation: 10
        reserved_cycles_limit: 10000000000000
        environment_variables:
          LOG_LEVEL: "warn"
          CACHE_TTL: "300"

  # Production environment - live deployment
  - name: prod
    network: production
    canisters: [frontend, backend]  # Database separate for prod
    settings:
      frontend:
        memory_allocation: 8589934592  # 8GB
        compute_allocation: 20
        freezing_threshold: 7776000    # 90 days
        wasm_memory_limit: 4294967296  # 4GB
        environment_variables:
          NODE_ENV: "production"
          API_BASE_URL: "https://ic0.app"
      backend:
        compute_allocation: 30
        reserved_cycles_limit: 50000000000000
        wasm_memory_limit: 2147483648  # 2GB
        environment_variables:
          LOG_LEVEL: "error"
          CACHE_TTL: "3600"
```

### Deployment Commands

```bash
# Development
icp deploy --environment dev

# Testing  
icp deploy --environment test

# Staging
icp deploy --environment staging

# Production
icp deploy --environment prod
```

## Recipe Development Workflows

### Custom Recipe Creation

**1. Create recipe template:**
```yaml
# recipes/rust-service.hb.yaml
canister:
  name: {{configuration.name}}
  build:
    steps:
      - type: script
        commands:
          - cargo build --package {{configuration.package}} --target wasm32-unknown-unknown {{#if configuration.release}}--release{{/if}}
          - mv target/wasm32-unknown-unknown/{{#if configuration.release}}release{{else}}debug{{/if}}/{{configuration.package}}.wasm "$ICP_WASM_OUTPUT_PATH"
  
  {{#if configuration.settings}}
  settings:
    {{#each configuration.settings}}
    {{@key}}: {{this}}
    {{/each}}
  {{/if}}
```

**2. Use custom recipe:**
```yaml
# services/api/canister.yaml
canister:
  recipe:
    type: file://../../recipes/rust-service.hb.yaml
    configuration:
      name: api
      package: api-service
      release: true
      settings:
        compute_allocation: 10
        memory_allocation: 4294967296
```
