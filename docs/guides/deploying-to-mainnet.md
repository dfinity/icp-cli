# Deploying to Mainnet

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
icp cycles balance --ic

# Convert ICP to cycles (if you have ICP)
icp cycles mint --icp 1 --ic
```

**How many cycles do you need?**
- Creating a canister: ~100B cycles (0.1T)
- Simple backend: 1-5T cycles lasts weeks to months
- Start with 1-2T cycles and top up as needed

For detailed information on acquiring ICP, converting to cycles, and managing balances, see [Tokens and Cycles](tokens-and-cycles.md).

## Deploying

Deploy to mainnet using the `--ic` flag:

```bash
icp deploy --ic
```

This will:
1. Build your canisters
2. Create canisters on mainnet (if first deployment)
3. Install your WASM code
4. Run any sync steps (e.g., asset uploads)

### Deploying Specific Canisters

Deploy only certain canisters:

```bash
icp deploy frontend --ic
```

### Using Environments

If you've configured environments in your `icp.yaml`:

```bash
icp deploy --environment production
```

See [Managing Environments](managing-environments.md) for setup details.

## Verifying Deployment

List canisters configured in this environment:

```bash
icp canister list --ic
```

Check canister status:

```bash
icp canister status my-canister --ic
```

Call a method to verify it's working:

```bash
icp canister call my-canister greet '("World")' --ic
```

## Updating Deployed Canisters

After making changes, redeploy:

```bash
icp deploy --ic
```

This rebuilds and upgrades your existing canisters, preserving their state.

## Managing Canister Settings

View current settings:

```bash
icp canister settings show my-canister --ic
```

Update settings:

```bash
icp canister settings update my-canister --freezing-threshold 2592000 --ic
```

## Topping Up Cycles

Monitor canister cycles and top up when needed:

```bash
# Check canister cycles balance
icp canister status my-canister --ic

# Top up with 1 trillion cycles
icp canister top-up my-canister --amount 1000000000000 --ic
```

See [Tokens and Cycles](tokens-and-cycles.md) for more on managing cycles.

## Troubleshooting

**"Insufficient cycles"**

Your canister needs more cycles. Top up using:

```bash
icp canister top-up my-canister --amount 1000000000000 --ic
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

**Deployment hangs**

Check network connectivity:

```bash
icp network ping mainnet
```

## Next Steps

- [Tokens and Cycles](tokens-and-cycles.md) — Managing ICP and cycles in detail
- [Managing Environments](managing-environments.md) — Set up staging and production

[Browse all documentation →](../index.md)
