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

# Production (with confirmation)
icp deploy --environment prod --ic
```

### Environment-Specific Configuration

Use Handlebars templating for dynamic configuration:

```yaml
# canister.yaml
canister:
  name: api-service
  build:
    steps:
      - type: script
        commands:
          - cargo build --target wasm32-unknown-unknown --release {{#if (eq environment "prod")}}--features production{{/if}}
          - mv target/wasm32-unknown-unknown/release/api.wasm "$ICP_WASM_OUTPUT_PATH"
  
  settings:
    memory_allocation: {{#switch environment}}
      {{#case "dev"}}1073741824{{/case}}
      {{#case "staging"}}4294967296{{/case}}
      {{#case "prod"}}8589934592{{/case}}
    {{/switch}}
```

## CI/CD Integration

### GitHub Actions

```yaml
# .github/workflows/deploy.yml
name: Deploy to IC

on:
  push:
    branches: [main, staging, develop]
  pull_request:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust
        uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
          target: wasm32-unknown-unknown
      
      - name: Install dfx
        run: |
          wget https://github.com/dfinity/sdk/releases/latest/download/dfx-linux-x86_64.tar.gz
          tar -xf dfx-linux-x86_64.tar.gz
          sudo mv dfx /usr/local/bin/
          dfx cache install

      - name: Build ICP CLI
        run: |
          cargo build --release
          echo "$(pwd)/target/release" >> $GITHUB_PATH
      
      - name: Setup environment
        run: |
          export ICPTEST_DFX_PATH="$(dfx cache show)/dfx"
          export ICP_POCKET_IC_PATH="$(dfx cache show)/pocket-ic"
      
      - name: Run tests
        run: |
          icp build --environment test
          icp network run --detach
          icp deploy --environment test
          # Run integration tests here
  
  deploy-staging:
    needs: test
    runs-on: ubuntu-latest
    if: github.ref == 'refs/heads/staging'
    steps:
      - uses: actions/checkout@v4
      
      - name: Setup deployment
        run: |
          # Setup ICP CLI and dependencies
          
      - name: Import identity
        env:
          STAGING_IDENTITY_PEM: ${{ secrets.STAGING_IDENTITY_PEM }}
        run: |
          echo "$STAGING_IDENTITY_PEM" > staging.pem
          icp identity import staging --from-pem staging.pem
          icp identity default staging
      
      - name: Deploy to staging
        run: |
          icp deploy --environment staging
  
  deploy-production:
    needs: test
    runs-on: ubuntu-latest
    if: github.ref == 'refs/heads/main'
    environment: production  # Requires manual approval
    steps:
      - uses: actions/checkout@v4
      
      - name: Setup deployment
        run: |
          # Setup ICP CLI and dependencies
          
      - name: Import production identity
        env:
          PROD_IDENTITY_PEM: ${{ secrets.PROD_IDENTITY_PEM }}
        run: |
          echo "$PROD_IDENTITY_PEM" > prod.pem
          icp identity import production --from-pem prod.pem
          icp identity default production
      
      - name: Deploy to production
        run: |
          icp deploy --environment prod --ic
```

### GitLab CI

```yaml
# .gitlab-ci.yml
stages:
  - test
  - deploy-staging
  - deploy-production

variables:
  CARGO_HOME: $CI_PROJECT_DIR/.cargo

before_script:
  - apt-get update && apt-get install -y wget
  - curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  - source ~/.cargo/env
  - rustup target add wasm32-unknown-unknown

test:
  stage: test
  script:
    - cargo build --release
    - export PATH=$PWD/target/release:$PATH
    - icp build --dry-run
  artifacts:
    paths:
      - target/release/icp

deploy-staging:
  stage: deploy-staging
  only:
    - staging
  before_script:
    - echo "$STAGING_IDENTITY_PEM" | base64 -d > staging.pem
    - target/release/icp identity import staging --from-pem staging.pem
    - target/release/icp identity default staging
  script:
    - target/release/icp deploy --environment staging

deploy-production:
  stage: deploy-production
  only:
    - main
  when: manual  # Requires manual trigger
  before_script:
    - echo "$PROD_IDENTITY_PEM" | base64 -d > prod.pem
    - target/release/icp identity import production --from-pem prod.pem
    - target/release/icp identity default production
  script:
    - target/release/icp deploy --environment prod --ic
```

## Complex Project Structures

### Monorepo with Multiple Services

```
project/
├── icp.yaml              # Root project configuration
├── services/
│   ├── api/
│   │   ├── canister.yaml
│   │   ├── src/
│   │   └── Cargo.toml
│   ├── frontend/
│   │   ├── canister.yaml
│   │   ├── dist/
│   │   └── package.json
│   └── database/
│       ├── canister.yaml
│       └── src/
├── shared/
│   ├── types/
│   └── utils/
└── infrastructure/
    ├── networks/
    │   ├── staging.yaml
    │   └── production.yaml
    └── scripts/
```

**Root configuration:**
```yaml
# icp.yaml
canisters:
  - services/*/canister.yaml

networks:
  - infrastructure/networks/*.yaml

environments:
  - name: dev
    network: local
    canisters: [api, frontend, database]
  
  - name: staging  
    network: staging
    canisters: [api, frontend, database]
    
  - name: prod
    network: production
    canisters: [api, frontend]  # Database deployed separately
```

### Microservice Architecture

```yaml
# services/api/canister.yaml
canister:
  name: api
  build:
    steps:
      - type: script
        commands:
          - cd ../../  # Return to project root
          - cargo build --package api --target wasm32-unknown-unknown --release
          - mv target/wasm32-unknown-unknown/release/api.wasm "$ICP_WASM_OUTPUT_PATH"
  
  settings:
    environment_variables:
      DATABASE_CANISTER_ID: "{{canister_id 'database'}}"
      FRONTEND_CANISTER_ID: "{{canister_id 'frontend'}}"
```

### Cross-Canister Communication

```yaml
# services/frontend/canister.yaml  
canister:
  name: frontend
  build:
    steps:
      - type: script
        commands:
          - npm ci
          - npm run build
      - type: assets
        source: dist
        target: /
  
  sync:
    steps:
      - type: assets
        source: dist  
        target: /
  
  settings:
    environment_variables:
      API_CANISTER_ID: "{{canister_id 'api'}}"
      NETWORK_HOST: "{{network.gateway.host}}"
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

### Recipe Registry Workflow

**1. Publish recipe to registry:**
```bash
# Upload to your recipe registry
curl -X POST https://recipes.mycompany.com/api/recipes \
  -H "Content-Type: application/yaml" \
  --data-binary @rust-service.hb.yaml
```

**2. Use remote recipe:**
```yaml
canister:
  recipe:
    type: https://recipes.mycompany.com/rust-service/v2.yaml
    configuration:
      name: api
      package: api-service
```

## Advanced Build Patterns

### Conditional Compilation

```yaml
canister:
  name: my-service
  build:
    steps:
      - type: script
        commands:
          # Conditional features based on environment
          - |
            FEATURES=""
            {{#if (eq environment "prod")}}
            FEATURES="--features production,optimized"
            {{else if (eq environment "staging")}}
            FEATURES="--features staging,metrics"
            {{else}}
            FEATURES="--features development,debug"
            {{/if}}
            cargo build --target wasm32-unknown-unknown --release $FEATURES
          - mv target/wasm32-unknown-unknown/release/my_service.wasm "$ICP_WASM_OUTPUT_PATH"
```

### Multi-Stage Builds

```yaml
canister:
  name: optimized-frontend
  build:
    steps:
      # Stage 1: Install dependencies
      - type: script  
        commands:
          - npm ci --production=false
      
      # Stage 2: Build application
      - type: script
        commands:
          - npm run build:{{environment}}
          - npm run optimize
      
      # Stage 3: Bundle assets
      - type: assets
        source: dist
        target: /
        exclude_patterns:
          - "*.map"
          - "test/**"
          - "docs/**"
```

### Build Caching

```yaml
canister:
  name: cached-build
  build:
    steps:
      - type: script
        commands:
          # Use build cache when available
          - |
            if [ -f ".build-cache/{{git_sha}}/my_service.wasm" ]; then
              echo "Using cached build"
              cp ".build-cache/{{git_sha}}/my_service.wasm" "$ICP_WASM_OUTPUT_PATH"
            else
              echo "Building from source"
              cargo build --target wasm32-unknown-unknown --release
              mkdir -p ".build-cache/{{git_sha}}"
              cp target/wasm32-unknown-unknown/release/my_service.wasm ".build-cache/{{git_sha}}/"
              cp ".build-cache/{{git_sha}}/my_service.wasm" "$ICP_WASM_OUTPUT_PATH"  
            fi
```

## Security and Identity Management

### Multiple Identity Strategy

```bash
# Development identity
icp identity new dev-alice
icp identity default dev-alice

# Staging identity (imported from secure storage)
icp identity import staging --from-pem staging-key.pem

# Production identity (hardware wallet or secure enclave)
icp identity import production --from-pem production-key.pem --assert-key-type secp256k1
```

### Environment-Specific Identity Usage

```yaml
# .github/workflows/deploy.yml
- name: Select deployment identity
  run: |
    case "${{ github.ref }}" in
      refs/heads/develop)
        echo "$DEV_IDENTITY_PEM" | base64 -d > dev.pem
        icp identity import dev --from-pem dev.pem
        icp identity default dev
        ;;
      refs/heads/staging)  
        echo "$STAGING_IDENTITY_PEM" | base64 -d > staging.pem
        icp identity import staging --from-pem staging.pem
        icp identity default staging
        ;;
      refs/heads/main)
        echo "$PROD_IDENTITY_PEM" | base64 -d > prod.pem
        icp identity import production --from-pem prod.pem
        icp identity default production
        ;;
    esac
```

### Controller Management

```yaml
environments:
  - name: prod
    network: production
    canisters: [api, frontend]
    # Multi-sig controller setup
    controller_settings:
      api:
        controllers:
          - "{{identity 'production'}}"
          - "{{identity 'backup'}}"  
          - "{{identity 'emergency'}}"
      frontend:
        controllers:
          - "{{identity 'production'}}"
          - "{{identity 'backup'}}"
```

## Monitoring and Observability

### Health Checks

```yaml
canister:
  name: api-service
  build:
    steps:
      - type: script
        commands:
          - cargo build --target wasm32-unknown-unknown --release --features health-checks
          - mv target/wasm32-unknown-unknown/release/api.wasm "$ICP_WASM_OUTPUT_PATH"
  
  # Post-deployment health verification
  sync:
    steps:
      - type: script
        commands:
          - |
            echo "Waiting for canister to initialize..."
            sleep 5
            
            # Health check with retry
            for i in {1..10}; do
              if icp canister call {{canister.name}} health_check '()'; then
                echo "Health check passed"
                break
              else
                echo "Health check failed, attempt $i/10"
                sleep 2
              fi
            done
```

### Deployment Verification

```bash
#!/bin/bash
# scripts/verify-deployment.sh

set -e

ENVIRONMENT=${1:-staging}
echo "Verifying deployment in $ENVIRONMENT environment..."

# Check all canisters are running
for canister in $(icp canister list --environment $ENVIRONMENT | cut -d' ' -f1); do
  echo "Checking $canister status..."
  status=$(icp canister status $canister --environment $ENVIRONMENT)
  if [[ $status != *"Running"* ]]; then
    echo "ERROR: $canister is not running"
    exit 1
  fi
done

# Run smoke tests
echo "Running smoke tests..."
icp canister call api health_check '()' --environment $ENVIRONMENT
icp canister call frontend get_version '()' --environment $ENVIRONMENT

echo "Deployment verification completed successfully"
```

## Performance Optimization

### Parallel Builds

```bash
# Build multiple canisters in parallel
icp build api frontend database &
wait

# Or use job control for complex builds
{
  icp build api --parallel &
  icp build frontend --parallel &
  icp build database --parallel &
  wait
}
```

### Resource Management

```yaml
environments:
  - name: high-performance
    network: production
    canisters: [api]
    settings:
      api:
        # Maximum performance configuration
        compute_allocation: 100
        memory_allocation: 17179869184  # 16GB
        wasm_memory_limit: 4294967296   # 4GB
        reserved_cycles_limit: 100000000000000
        
        # Optimize for low latency
        wasm_memory_threshold: 3221225472  # 3GB (trigger early GC)
```

These advanced workflows enable you to build robust, scalable Internet Computer applications with proper DevOps practices, security considerations, and performance optimization.
