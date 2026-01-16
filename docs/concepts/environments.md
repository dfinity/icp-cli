# Environments and Networks

Understanding the relationship between networks and environments is key to effective deployment management.

## Networks

A **network** is an ICP network endpoint that icp-cli can connect to.

### Network Types

**Managed Networks**

icp-cli controls the lifecycle — starting, stopping, and resetting:

```yaml
networks:
  - name: local
    mode: managed
    gateway:
      host: 127.0.0.1
      port: 8000
```
Managed networks can run natively on your machine or inside a docker container.

Use managed networks for local development and testing.

**Connected Networks**

External networks that icp-cli connects to but doesn't control:

```yaml
networks:
  - name: testnet
    mode: connected
    url: https://testnet.ic0.app
```

Use connected networks for shared testnets and production.

### Implicit Networks

Two networks are always available:

| Network | Type | Description |
|---------|------|-------------|
| `local` | Managed | Local development network on `localhost:8000` |
| `mainnet` | Connected | The Internet Computer mainnet |

The `local` network can be overridden. The `mainnet` network is protected and cannot be overridden.

### Overriding Local

Customize your local development network:

```yaml
networks:
  - name: local
    mode: managed
    gateway:
      port: 9999  # Different port
```

Or connect to an existing network instead of managing one:

```yaml
networks:
  - name: local
    mode: connected
    url: http://192.168.1.100:8000
    root-key: <hex-encoded-root-key>
```

## Environments

An **environment** is a named deployment target that combines:

- A **network** to deploy to
- A set of **canisters** to include
- **Settings** for those canisters

### Why Environments?

Without environments, you'd need to:
- Remember which network to deploy to
- Manually specify settings for each deployment
- Track canister IDs separately

Environments encapsulate all of this.

### Implicit Environments

Two environments are always available:

| Environment | Network | Canisters |
|-------------|---------|-----------|
| `local` | `local` | All canisters |
| `ic` | `mainnet` | All canisters |

### Defining Environments

```yaml
environments:
  - name: staging
    network: mainnet
    canisters: [frontend, backend]
    settings:
      backend:
        compute_allocation: 5

  - name: production
    network: mainnet
    canisters: [frontend, backend]
    settings:
      backend:
        compute_allocation: 20
        freezing_threshold: 7776000
```

### Environment-Specific Settings

Settings cascade with environment overrides taking precedence:

```yaml
canisters:
  - name: backend
    settings:
      compute_allocation: 1  # Default

environments:
  - name: staging
    network: mainnet
    canisters: [backend]

  - name: production
    network: mainnet
    canisters: [backend]
    settings:
      backend:
        compute_allocation: 20  # Override for production
```

### Using Environments

```bash
# Local development (default)
icp deploy

# Explicit local
icp deploy --environment local

# Custom environment
icp deploy --e staging
```

## Networks vs Environments

| Aspect | Network | Environment |
|--------|---------|-------------|
| **Purpose** | Where to connect | What to deploy and how |
| **Contains** | URL, connection details | Network reference, canisters, settings |
| **Examples** | `local`, `mainnet`, `testnet` | `local`, `ic`, `staging`, `production` |

A common pattern:

```
Networks: local, mainnet
Environments: local, staging, production
                 ↓        ↓         ↓
              local   mainnet   mainnet
```

Multiple environments can target the same network with different settings.

## Canister IDs per Environment

Each environment maintains separate canister IDs. The storage location depends on network type:

- **Managed networks** (local): `.icp/cache/mappings/<environment>.ids.json`
- **Connected networks** (mainnet): `.icp/data/mappings/<environment>.ids.json`

**IMPORTANT** Creating canisters on mainnet is like buying real-estate so you should make sure
not to lose the canister ids. It is common practice to checkin the contents of `.icp/data` in
source control so as not to lose them.

## Next Steps

- [Managing Environments](../guides/managing-environments.md) — Apply this in practice

[Browse all documentation →](../index.md)
