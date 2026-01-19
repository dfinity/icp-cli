# Deploying to IC Mainnet

This guide walks through deploying your canisters to the Internet Computer mainnet.

## Prerequisites

Before deploying to mainnet, ensure you have:

1. **A working project** — Test locally first with `icp deploy` on your local network
2. **An identity** — See [Managing Identities](managing-identities.md)
3. **Cycles** — Canisters require cycles to run on mainnet

## Setting Up an Identity

If you haven't already, create an identity:

```bash
icp identity new mainnet-deployer
```

Set it as default:

```bash
icp identity default mainnet-deployer
```

View your principal:

```bash
icp identity principal
```

## Acquiring Cycles

Canisters need cycles to operate on mainnet. You'll need cycles before deploying.

**Quick start:**

```bash
# Check your cycles balance
icp cycles balance -e ic

# Convert ICP to cycles (if you have ICP)
icp cycles mint --icp 1 -e ic
```

**How many cycles do you need?**
- Creating a canister: ~100B cycles (0.1T)
- Simple backend: 1-5T cycles lasts weeks to months
- Start with 1-2T cycles and top up as needed

For detailed information on acquiring ICP, converting to cycles, and managing balances, see [Tokens and Cycles](tokens-and-cycles.md).

## Deploying

To deploy to the IC mainnet, use the implicit `ic` environment with the `--environment ic` flag or the `-e ic` shorthand:

```bash
icp deploy --environment ic
```

This will:
1. Build your canisters
2. Create canisters on mainnet (if first deployment)
3. Install your WASM code
4. Run any sync steps (e.g., asset uploads)

### Deploying Specific Canisters

Deploy only certain canisters:

```bash
icp deploy frontend --environment ic
```

### Using Environments

You can configure multiple environments pointing to the IC mainnet in `icp.yaml`:

```yaml

environments:
  - name: prod
    network: ic  # ic is an implicit network
  - name: staging
    network: ic
```
This allows you to deploy independent sets of canisters for each environment:

```bash
icp deploy -e staging
icp deploy --environment prod
```

See [Managing Environments](managing-environments.md) for setup details.

## Verifying Deployment

List canisters configured in this environment:

```bash

# List the canisters in an environment
icp canister list -e myenv

# Check canister status:
icp canister status my-canister -e myenv

# Call a method to verify it's working:
icp canister call my-canister greet '("World")' -e myenv
```

## Updating Deployed Canisters

After making changes, redeploy:

```bash
icp deploy --environment prod
```

This rebuilds and upgrades your existing canisters, preserving their state.

## Managing Canister Settings

View current settings:

```bash
icp canister settings show my-canister -e prod
```

Update settings:

```bash
icp canister settings update my-canister --freezing-threshold 2592000 -e prod
```

## Topping Up Cycles

Monitor canister cycles and top up when needed:

```bash
# Check canister cycles balance
icp canister status my-canister -e prod

# Top up with 1 trillion cycles
icp canister top-up my-canister --amount 1000000000000 -e prod
```

See [Tokens and Cycles](tokens-and-cycles.md) for more on managing cycles.

## Troubleshooting

**"Insufficient cycles"**

Your canister needs more cycles. Top up using:

```bash
icp canister top-up my-canister --amount 1000000000000 -e prod
```

**"Not a controller"**

You're not authorized to modify this canister. Verify you're using the correct identity:

```bash
icp identity principal
icp identity list
```

If needed, switch to the correct identity:

```bash
icp identity default <identity-name>
```

## Next Steps

- [Tokens and Cycles](tokens-and-cycles.md) — Managing ICP and cycles in detail
- [Managing Environments](managing-environments.md) — Set up staging and production

[Browse all documentation →](../index.md)
