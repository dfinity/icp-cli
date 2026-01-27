# Tokens and Cycles

This is a command reference for managing ICP tokens and cycles. If you're deploying to mainnet for the first time, start with [Deploying to Mainnet](deploying-to-mainnet.md) instead.

The Internet Computer uses two primary currencies:

| Currency | Purpose | Used For |
|----------|---------|----------|
| **ICP** | Governance token | Trading, staking, converting to cycles |
| **Cycles** | Computational fuel | Running canisters, paying for storage and compute |

Canisters consume cycles to operate on mainnet. To obtain cycles, you convert ICP tokens at the current ICP/XDR exchange rate. XDR (Special Drawing Rights) is an international reserve asset used for stable pricing. One trillion cycles (1T) costs approximately 1 XDR worth of ICP.

## Amount Format

All cycle and token amounts support human-readable suffixes for convenience:
- `k` or `K` = thousand (1,000)
- `m` or `M` = million (1,000,000)
- `b` or `B` = billion (1,000,000,000)
- `t` or `T` = trillion (1,000,000,000,000)

Suffixes are case-insensitive. Examples: `1.5T`, `500m`, `1_234.5B`, `2K`

## Network and Environment Flags

Understanding when to use `-n` (network) vs `-e` (environment) is essential:

| Flag | Purpose | Used With | Example |
|------|---------|-----------|---------|
| `-n ic` | Network flag | Token and cycles operations | `icp token balance -n ic`<br>`icp cycles mint -n ic` |
| `-e ic` | Environment flag | Deployment and canister operations | `icp deploy -e ic`<br>`icp canister status my-canister -e ic` |

**For canister operations:**

The flag you use depends on whether you're referencing canisters by name or by ID:

- **Canister names** (like `my-canister`) — Must use `-e <environment>`
  - Environment knows about your project's canister mappings
  - Examples: `icp canister status my-canister -e ic`

- **Canister IDs** (like `ryjl3-tyaaa-aaaaa-aaaba-cai`) — Can use either `-e` or `-n`
  - Use `-e` when working within your project context
  - Use `-n` when working with arbitrary canisters on a network
  - Examples: `icp canister status ryjl3-tyaaa-aaaaa-aaaba-cai -n ic`

```bash
# ✓ Works - canister name with environment
icp canister status my-canister -e ic
icp canister top-up my-canister --amount 1T -e ic

# ✗ Fails - canister name with network (no project context)
icp canister status my-canister -n ic

# ✓ Works - canister ID with network
icp canister status ryjl3-tyaaa-aaaaa-aaaba-cai -n ic
icp canister top-up ryjl3-tyaaa-aaaaa-aaaba-cai --amount 1T -n ic

# ✓ Also works - canister ID with environment
icp canister status ryjl3-tyaaa-aaaaa-aaaba-cai -e ic
```

## Checking Balances

Check your ICP balance:

```bash
# On IC mainnet
icp token balance -n ic

# On local network (for testing)
icp token balance
```

Check your cycles balance:

```bash
# On IC mainnet
icp cycles balance -n ic

# On local network
icp cycles balance
```

Check a canister's cycles balance:

```bash
# By canister name (in your project environment)
icp canister status my-canister -e ic

# By canister ID (on any network)
icp canister status ryjl3-tyaaa-aaaaa-aaaba-cai -n ic
```

The output includes the canister's cycles balance.

## Converting ICP to Cycles

Convert ICP tokens to cycles for use with canisters:

```bash
# Convert a specific amount of ICP
icp cycles mint --icp 5 -n ic

# Or request a specific amount of cycles (ICP calculated automatically)
icp cycles mint --cycles 5T -n ic
```

Verify your cycles balance:

```bash
icp cycles balance -n ic
```

## Transferring ICP Tokens

Send ICP tokens to another principal:

```bash
# The 'icp' token is used by default
icp token transfer <AMOUNT> <RECEIVER> -n ic

# Explicitly specifying 'icp' is equivalent
icp token icp transfer <AMOUNT> <RECEIVER> -n ic
```

Examples:

```bash
# Send 1 ICP
icp token transfer 1 aaaaa-aa -n ic

# Send 0.5 ICP
icp token transfer 0.5 xxxxx-xxxxx-xxxxx-xxxxx-cai -n ic

# Using human-readable amounts
icp token transfer 1.5m xxxxx-xxxxx-xxxxx-xxxxx-cai -n ic
```

The receiver can be a principal ID or canister ID.

### Getting Account Identifiers

To get your ICP ledger account identifier (for transfers to/from exchanges or wallets that don't support principals yet):

```bash
icp identity account-id
```

See [Managing Identities](managing-identities.md) for more details on account identifiers.

## Working with ICRC-1 Tokens

icp-cli supports ICRC-1 tokens by specifying the token's ledger canister ID. ICRC-1 is a fungible token standard on the Internet Computer, which means all ICRC-1 tokens work with the same commands.

To transfer ICRC-1 tokens, specify the ledger canister ID. Example with ckBTC (ledger: `mxzaz-hqaaa-aaaar-qaada-cai`):

```bash
# Check ckBTC balance
icp token mxzaz-hqaaa-aaaar-qaada-cai balance -n ic

# Transfer 0.001 ckBTC
icp token mxzaz-hqaaa-aaaar-qaada-cai transfer 0.001 xxxxx-xxxxx-xxxxx-xxxxx-cai -n ic
```

This works with any ICRC-1 compatible token ledger on the Internet Computer.

**Finding Token Ledger IDs:** You can find ledger canister IDs for various tokens on the [ICP Dashboard](https://dashboard.internetcomputer.org/tokens).

## Transferring Cycles

Transfer cycles directly to another principal via the cycles ledger:

```bash
icp cycles transfer <AMOUNT> <RECEIVER> -n ic
```

Examples:

```bash
# Transfer 1 trillion cycles
icp cycles transfer 1T aaaaa-aa -n ic

# Transfer 500 million cycles
icp cycles transfer 500m xxxxx-xxxxx-xxxxx-xxxxx-cai -n ic
```

The receiver can be a principal ID or canister ID.

## Monitoring Canister Cycles

Regularly check canister cycles to avoid running out. **If a canister runs out of cycles, it will be frozen and eventually deleted along with all its code and state.**

```bash
# Check all canisters in an environment
icp canister status -e ic

# Check specific canister by name (in your project)
icp canister status my-canister -e ic

# Check specific canister by ID (on any network)
icp canister status ryjl3-tyaaa-aaaaa-aaaba-cai -n ic
```

## Topping Up Canisters

Add cycles to a canister to keep it running:

```bash
# Top up by canister name (in your project)
icp canister top-up my-canister --amount 1T -e ic

# Top up by canister ID (on any network)
icp canister top-up ryjl3-tyaaa-aaaaa-aaaba-cai --amount 1T -n ic
```

The `--amount` is specified in cycles (not ICP) and supports human-readable suffixes like `1T`, `500m`, etc.

## Cycles Transfer vs Canister Top-Up

Understanding the difference between these two commands helps you choose the right one for your use case.

The **cycles ledger** is an ICP system canister that tracks cycles balances for principals, similar to how the ICP ledger tracks ICP token balances.

There are two ways to send cycles:

| Command | Destination | Use Case |
|---------|-------------|----------|
| `icp cycles transfer` | Recipient's **cycles balance** | Transfer cycles to another principal's cycles ledger balance (similar to sending tokens) |
| `icp canister top-up` | Canister's **operating balance** | Add cycles directly to a canister to pay for its compute and storage |

**When to use each:**
- **`cycles transfer`**: Send cycles to another person/identity through the cycles ledger, similar to how you transfer ICP tokens
- **`canister top-up`**: Directly fund a canister to keep it running (most common for maintaining canisters)

## Using Different Identities

Specify which identity to use for token operations:

```bash
# Check balance for a specific identity
icp token balance --identity my-other-identity -n ic

# Transfer using a specific identity
icp token transfer 1 <RECEIVER> --identity my-wallet -n ic
```

See [Managing Identities](managing-identities.md) for more details.

## Fees and Safety

### Transaction Fees

All transfers incur small fees:
- **ICP transfers**: 0.0001 ICP fee
- **Cycles transfers**: Small fee in cycles (varies by operation)
- **ICRC-1 tokens**: Fee varies by token (typically minimal)

Fees are automatically deducted from your balance when you initiate a transfer.

### Safety Considerations

**Transfers are irreversible.** Once sent, transactions cannot be undone. To minimize risk:

- **Verify receiver addresses** — Double-check the principal or canister ID before sending
- **Test with small amounts** — For large transfers or new recipients, send a small test amount first
- **Confirm the target network** — `-n ic` targets IC mainnet; omitting it uses your local network

**Insufficient balance:** If you don't have enough funds (including fees), the transfer will fail with an error.

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
# By canister name (in your project environment)
icp canister top-up my-canister --amount 1T -e ic

# By canister ID (on any network)
icp canister top-up ryjl3-tyaaa-aaaaa-aaaba-cai --amount 1T -n ic
```

**Transfer fails**

Verify:
- The receiver address is correct
- You have sufficient balance (including fees)
- You're using the correct identity

## Next Steps

- [Deploying to Mainnet](deploying-to-mainnet.md) — Complete mainnet deployment guide
- [Managing Identities](managing-identities.md) — Manage keys and principals

[Browse all documentation →](../index.md)
