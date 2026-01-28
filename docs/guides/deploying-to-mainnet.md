# Deploying to IC Mainnet

This guide walks through deploying your canisters to the Internet Computer mainnet.

## Understanding Mainnet Deployment

Unlike local development (which has unlimited resources), deploying to mainnet requires paying for computation and storage.

**Key concepts:**

- **Identity** — Your cryptographic identity on the Internet Computer
  - Represented by a **principal** (a unique identifier like `aaaaa-aa`)
  - Think of it like your public address for receiving tokens
  - Your identity will be the controller (owner) of your canisters, allowing you to deploy, update, and manage them

- **ICP tokens** — The Internet Computer's governance token
  - Purchase from cryptocurrency exchanges or receive from others
  - You'll convert ICP to cycles to power your canisters

- **Cycles** — Computational fuel that powers canisters
  - Canisters consume cycles for compute and storage (similar to cloud hosting costs)
  - Convert ICP to cycles before deploying

**Network flags you'll see:**
- `-n ic` = network flag for token and cycles operations (e.g., `icp token balance -n ic`, `icp cycles mint -n ic`)
- `-e ic` = environment flag for deployment and canister operations (e.g., `icp deploy -e ic`, `icp canister status -e ic`)

**Important:** When working with your project's canisters by name (like `my-canister`), you must use `-e`. The `-n` flag only works with canister IDs (like `ryjl3-tyaaa-aaaaa-aaaba-cai`).

**Amount format:** Amounts use human-readable suffixes throughout:
- `T` = trillion (1,000,000,000,000)
- `m` = million, `b` = billion, `k` = thousand
- Examples: `5T` = 5 trillion cycles, `0.5` = half an ICP token

## Prerequisites

Before deploying to mainnet, ensure you have:

1. **A working project** — Test locally first with `icp deploy` on your local network
2. **An identity** — You'll create one in this guide
3. **ICP tokens** — You'll acquire these in this guide

The following sections walk through each step. For experienced users, see the [Complete Mainnet Workflow](#complete-mainnet-workflow) at the end.

## Setting Up an Identity

Create an identity for mainnet deployments. This generates a cryptographic key pair that represents you on the Internet Computer.

```bash
icp identity new mainnet-deployer
```

**⚠️ IMPORTANT:** Save the seed phrase displayed — it's shown only once and is required to restore your identity. Store it securely offline. Without it, you'll permanently lose access to your identity and any ICP/cycles associated with it.

Set it as default:

```bash
icp identity default mainnet-deployer
```

View your principal (your unique identifier for receiving tokens):

```bash
icp identity principal
# Output: xxxxx-xxxxx-xxxxx-xxxxx-xxx (your principal)
```

Save this principal — you'll need it to receive ICP tokens.

## Acquiring Cycles

Now you need to get ICP tokens and convert them to cycles.

### Getting ICP

**To get ICP tokens (choose one method):**

- **Purchase ICP** — Buy ICP through cryptocurrency exchanges or wallets that support direct purchases (like OISY)
  - Use your principal when withdrawing or receiving ICP

  **Note:** Some cryptocurrency exchanges may not support principals yet. If your exchange requires an account identifier instead, use: `icp identity account-id`

- **Receive from another user** — Share your principal with the sender: `icp identity principal`

**Verify ICP arrived:**

```bash
icp token balance -n ic
```

**Recommended starting amount:** 5-10 ICP for your first deployment (converts to ~5-10T cycles).

### Converting ICP to Cycles

Convert your ICP tokens to cycles (remember: "T" = trillion):

```bash
# Convert 5 ICP to cycles
icp cycles mint --icp 5 -n ic

# Or request a specific amount of cycles (ICP calculated automatically)
icp cycles mint --cycles 5T -n ic
```

**Verify your cycles balance:**

```bash
icp cycles balance -n ic
# Output: ~5T cycles (5 trillion cycles)
```

**Budget guidance:** Budget 1-2T cycles per canister minimum for initial deployment.

For detailed command reference and advanced options, see [Tokens and Cycles](tokens-and-cycles.md).

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
icp deploy my-canister --environment ic
```

## Verifying Deployment

Check your deployment:

```bash
# List deployed canisters
icp canister list -e ic

# Check canister status
icp canister status my-canister -e ic

# Call a method to verify it's working
icp canister call my-canister greet '("World")' -e ic
```

## Updating Deployed Canisters

After making changes, redeploy:

```bash
icp deploy -e ic
```

This rebuilds and upgrades your existing canisters, preserving their state.

## Managing Canisters

This section covers advanced canister management tasks.

### Updating Settings

Canister settings control operational parameters like freezing threshold (how long a canister can run without cycles before freezing) and memory allocation.

View current settings:

```bash
icp canister settings show my-canister -e ic
```

Update settings (example shows setting freezing threshold to 30 days):

```bash
icp canister settings update my-canister --freezing-threshold 2592000 -e ic
```

See [Canister Settings](../reference/canister-settings.md) for all available settings.

### Managing Controllers

Controllers are principals authorized to manage a canister (deploy code, update settings, delete the canister). By default, your identity is the only controller.

Add another controller (useful for team access or backup):

```bash
icp canister settings update my-canister --add-controller <principal> -e ic
```

Remove a controller:

```bash
icp canister settings update my-canister --remove-controller <principal> -e ic
```

### Topping Up Cycles

Canisters consume cycles continuously for compute and storage. Monitor cycles regularly to prevent your canister from freezing.

Check canister cycles balance:

```bash
icp canister status my-canister -e ic
```

Top up with cycles when running low:

```bash
icp canister top-up my-canister --amount 1T -e ic
```

See [Tokens and Cycles](tokens-and-cycles.md) for more on managing cycles.

## Using Multiple Environments

For more complex workflows with staging and production environments, you can configure multiple environments in `icp.yaml`:

```yaml
environments:
  - name: staging
    network: ic
  - name: prod
    network: ic
```

Then deploy to each environment:

```bash
icp deploy -e staging
icp deploy -e prod
```

See [Managing Environments](managing-environments.md) for complete setup and best practices.

## Complete Mainnet Workflow

Here's the complete workflow for quick reference:

```bash
# 1. Create a dedicated mainnet identity
icp identity new mainnet-deployer
icp identity default mainnet-deployer

# 2. Get your principal (your unique identifier) to receive ICP tokens
icp identity principal
# Output example: xxxxx-xxxxx-xxxxx-xxxxx-xxx
# Share this principal with the sender (exchange or another user)
# Note: If your exchange requires an account identifier instead, use: icp identity account-id

# 3. Verify ICP arrived
icp token balance -n ic
# Output: 10 ICP

# 4. Convert ICP to cycles
icp cycles mint --icp 5 -n ic

# 5. Verify your cycles balance
icp cycles balance -n ic
# Output: ~5T cycles

# 6. Deploy your project to mainnet
icp deploy -e ic

# 7. Monitor your canister's cycles
icp canister status my-canister -e ic

# 8. Top up if needed
icp canister top-up my-canister --amount 2T -e ic
```

The sections above explain each step in detail.

## Troubleshooting

**"Insufficient cycles"**

Your canister needs more cycles. Top up using:

```bash
icp canister top-up my-canister --amount 1T -e ic
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
- [Deploying to Specific Subnets](deploying-to-specific-subnets.md) — Target European or specialized subnets
- [Managing Environments](managing-environments.md) — Set up staging and production

[Browse all documentation →](../index.md)
