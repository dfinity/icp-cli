# Tokens and Cycles

This guide covers managing ICP tokens and cycles with icp-cli.

The Internet Computer uses two types of currency:

| Currency | Purpose | Used For |
|----------|---------|----------|
| **ICP** | Governance token | Trading, staking, converting to cycles |
| **Cycles** | Computational fuel | Running canisters, paying for storage and compute |

Canisters consume cycles to operate. To deploy and run canisters on the IC mainnet, you need cycles.

## Getting ICP and Cycles

### On IC Mainnet

To get ICP tokens:

1. **Receive from another wallet** — Share your principal: `icp identity principal`
2. **Purchase on an exchange** — Buy ICP and withdraw to your principal

To get cycles:

1. **Convert ICP** — Use `icp cycles mint` after acquiring ICP
2. **Receive cycles** — Someone can transfer cycles to your principal via the cycles ledger

### On Local Network

Local networks have unlimited cycles for testing. The default identity is automatically funded.

## Converting ICP to Cycles

Convert ICP tokens to cycles for use with canisters:

```bash
# Convert a specific amount of ICP
icp cycles mint --icp 1 -n ic

# Or request a specific amount of cycles (ICP calculated automatically)
icp cycles mint --cycles 1000000000000 -n ic
```

The conversion rate is determined by the current ICP/XDR exchange rate. One trillion cycles (1T = 1,000,000,000,000) costs approximately 1 XDR worth of ICP.

## Topping Up Canisters

Add cycles to a canister to keep it running:

```bash
icp canister top-up <canister-id> --amount 1000000000000 -n ic
```

The `--amount` is specified in cycles (not ICP).

### Monitoring Cycles

Regularly check canister cycles to avoid running out:

```bash
# Check all canisters in an environment
icp canister status -e my-env

# Check specific canister
icp canister status my-canister -e my-env
```

## Checking Balances

### ICP Token Balance

Check your ICP balance:

```bash
# On IC mainnet
icp token balance -n ic

# On local network (for testing)
icp token balance
```

### Cycles Balance

Check your cycles balance:

```bash
# On IC mainnet
icp cycles balance -n ic

# On local network
icp cycles balance
```

### Canister Cycles Balance

Check how many cycles a specific canister has:

```bash
icp canister status <canister-id> -n ic
```

The output includes the canister's cycles balance.

## Transferring ICP

Send ICP tokens to another principal:

```bash
icp token transfer <AMOUNT> <RECEIVER> -n ic
```

Example:

```bash
# Send 1 ICP
icp token transfer 1 aaaaa-aa -n ic

# Send 0.5 ICP
icp token transfer 0.5 xxxxx-xxxxx-xxxxx-xxxxx-cai -n ic
```

The receiver can be a principal ID or canister ID.

**Note:** Account identifiers are not yet supported. Support will be added soon.

## Working with Different Tokens

icp-cli supports ICRC-1 tokens beyond ICP:

```bash
# Check balance of a specific token
icp token <TOKEN_CANISTER_ID> balance -n ic

# Transfer a specific token
icp token <TOKEN_CANISTER_ID> transfer 100 <RECEIVER> -n ic
```

Replace `<TOKEN_CANISTER_ID>` with the canister ID of the token ledger.

## Using Different Identities

Specify which identity to use for token operations:

```bash
# Check balance for a specific identity
icp token balance --identity my-other-identity -n ic

# Transfer using a specific identity
icp token transfer 1 <RECEIVER> --identity my-wallet -n ic
```

## Troubleshooting

**"Insufficient balance"**

Your account doesn't have enough ICP or cycles. Check your balance:

```bash
icp token balance -n ic
icp cycles balance -n ic
```

**"Canister out of cycles"**

Top up the canister:

```bash
# On IC mainnet
icp canister top-up <canister-id> --amount 1000000000000 -n ic

# In an environment called `prod-env`
icp canister top-up <canister-name> --amount 1000000000000 -e prod-env
```

**Transfer fails**

Verify:
- The receiver address is correct
- You have sufficient balance (including fees)
- You're using the correct identity

## Next Steps

- [Deploying to Mainnet](deploying-to-mainnet.md) — Use cycles to deploy canisters
- [Managing Identities](managing-identities.md) — Manage keys and principals

[Browse all documentation →](../index.md)
