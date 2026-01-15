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

Canisters need cycles to operate on mainnet. Cycles are the computational fuel for canisters — they're consumed when your canister executes code, stores data, or handles requests.

### Option 1: Receive Cycles Directly

Someone can transfer cycles to your principal via the cycles ledger. This is the simplest option if you know someone who already has cycles.

To receive cycles, share your principal:

```bash
icp identity principal
```

### Option 2: Convert ICP to Cycles

If you have ICP tokens, you can convert them to cycles. To get ICP:

- **Transfer from another wallet** — Receive ICP from someone who already has tokens
- **Buy on a secondary market** — Purchase ICP and withdraw to your principal

Note: Displaying the AccountIdentifier of your identity is not yet supported by icp-cli.

#### Converting ICP to Cycles

Once you have ICP tokens, convert them to cycles:

```bash
# Check your ICP balance
icp token balance --ic

# Convert 1 ICP to cycles
icp cycles mint --icp 1 --ic

# Or request a specific amount of cycles (ICP amount determined automatically)
icp cycles mint --cycles 1000000000000 --ic
```

The conversion rate is determined automatically based on the current ICP/XDR exchange rate. One trillion cycles (1T = 1,000,000,000,000) costs approximately 1 XDR worth of ICP.

### Check Your Cycles Balance

```bash
icp cycles balance --ic
```

### How Many Cycles Do You Need?

For getting started:
- **Creating a canister**: ~100B cycles (0.1T)
- **Simple backend canister**: 1-5T cycles lasts weeks to months depending on usage
- **Frontend with assets**: More storage means more cycles consumed

Start with 1-2T cycles for initial development and top up as needed.

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

Monitor cycles and top up when needed:

```bash
# Check balance
icp canister status my-canister --ic

# Top up with 1 trillion cycles
icp canister top-up my-canister --amount 1000000000000 --ic
```

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
icp network ping --network mainnet
```

## Next Steps

- [Managing Environments](managing-environments.md) — Set up staging and production

[Browse all documentation →](../index.md)
