# Tokens and Cycles

This guide covers managing ICP tokens and cycles with icp-cli.

## Overview

The Internet Computer uses two types of currency:

| Currency | Purpose | Used For |
|----------|---------|----------|
| **ICP** | Governance token | Trading, staking, converting to cycles |
| **Cycles** | Computational fuel | Running canisters, paying for storage and compute |

Canisters consume cycles to operate. To deploy and run canisters on mainnet, you need cycles.

## Checking Balances

### ICP Token Balance

Check your ICP balance:

```bash
# On mainnet
icp token balance --ic

# On local network (for testing)
icp token balance
```

### Cycles Balance

Check your cycles balance:

```bash
# On mainnet
icp cycles balance --ic

# On local network
icp cycles balance
```

### Canister Cycles Balance

Check how many cycles a specific canister has:

```bash
icp canister status my-canister --ic
```

The output includes the canister's cycles balance.

## Transferring ICP

Send ICP tokens to another principal:

```bash
icp token transfer <AMOUNT> <RECEIVER> --ic
```

Example:

```bash
# Send 1 ICP
icp token transfer 1 aaaaa-aa --ic

# Send 0.5 ICP
icp token transfer 0.5 xxxxx-xxxxx-xxxxx-xxxxx-cai --ic
```

The receiver can be a principal ID or account identifier.

## Converting ICP to Cycles

**Note:** You need ICP tokens before you can convert them to cycles. See [Getting ICP and Cycles](#getting-icp-and-cycles) below if you don't have ICP yet.

Convert ICP tokens to cycles for use with canisters:

```bash
# Convert a specific amount of ICP
icp cycles mint --icp 1 --ic

# Or request a specific amount of cycles (ICP calculated automatically)
icp cycles mint --cycles 1000000000000 --ic
```

The conversion rate is determined by the current ICP/XDR exchange rate. One trillion cycles (1T = 1,000,000,000,000) costs approximately 1 XDR worth of ICP.

## Topping Up Canisters

Add cycles to a canister to keep it running:

```bash
icp canister top-up my-canister --amount 1000000000000 --ic
```

The `--amount` is specified in cycles (not ICP).

### Cycles Amounts Reference

| Amount | Notation | Typical Use |
|--------|----------|-------------|
| 100,000,000,000 | 100B (0.1T) | Creating a canister |
| 1,000,000,000,000 | 1T | Small canister for weeks |
| 5,000,000,000,000 | 5T | Active canister for months |

### Monitoring Cycles

Regularly check canister cycles to avoid running out:

```bash
# Check all canisters in an environment
icp canister status --ic

# Check specific canister
icp canister status my-canister --ic
```

## Getting ICP and Cycles

### On Mainnet

To get ICP tokens:

1. **Receive from another wallet** — Share your principal: `icp identity principal`
2. **Purchase on an exchange** — Buy ICP and withdraw to your principal

To get cycles:

1. **Convert ICP** — Use `icp cycles mint` after acquiring ICP
2. **Receive cycles** — Someone can transfer cycles to your principal via the cycles ledger

### On Local Network

Local networks have unlimited cycles for testing. The default identity is automatically funded.

## Working with Different Tokens

icp-cli supports ICRC-1 tokens beyond ICP:

```bash
# Check balance of a specific token
icp token <TOKEN_CANISTER_ID> balance --ic

# Transfer a specific token
icp token <TOKEN_CANISTER_ID> transfer 100 <RECEIVER> --ic
```

Replace `<TOKEN_CANISTER_ID>` with the canister ID of the token ledger.

## Using Different Identities

Specify which identity to use for token operations:

```bash
# Check balance for a specific identity
icp token balance --identity my-other-identity --ic

# Transfer using a specific identity
icp token transfer 1 <RECEIVER> --identity my-wallet --ic
```

## Troubleshooting

**"Insufficient balance"**

Your account doesn't have enough ICP or cycles. Check your balance:

```bash
icp token balance --ic
icp cycles balance --ic
```

**"Canister out of cycles"**

Top up the canister:

```bash
icp canister top-up my-canister --amount 1000000000000 --ic
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
