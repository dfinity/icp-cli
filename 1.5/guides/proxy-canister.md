# Proxy Canister

This guide explains the proxy canister pattern, when you need it, and how to deploy and use a proxy on connected networks like IC mainnet.

## Why a Proxy?

The IC protocol imposes two constraints on external clients (CLI tools, scripts, browser apps):

1. **Clients cannot attach cycles to calls.** Only canisters can fund inter-canister calls with cycles. A canister method that charges cycles for execution is therefore unreachable directly from a CLI tool.

2. **Some management canister methods are canister-only.** Certain [`aaaaa-aa` management canister](https://docs.internetcomputer.org/references/management-canister) methods — like `canister_info` or `raw_rand` — can only be called by other canisters, not by external clients.

The proxy canister solves both constraints. It accepts a `proxy` method call from an authorized caller, then forwards the call to the target canister as a canister-to-canister call. Cycles are deducted from the proxy's own balance and attached to the forwarded call.

```mermaid
flowchart LR
    A[You] -->|call| B[Proxy canister]
    B -->|forward + cycles| C[Target canister]
```

## When You Need a Proxy

You need a proxy canister whenever you:

- **Attach cycles to a call** — for example, topping up a canister or calling a pay-per-use method.
- **Call a management canister method** that is restricted to canister callers (e.g. `canister_info`, `raw_rand`, `create_canister`, `install_chunked_code`).
- **Run icp-cli against IC mainnet** and need cycles-funded operations.

## Local Development: Automatic Proxy

On managed (local) networks, icp-cli automatically deploys a proxy canister and seeds it with cycles. You can use it immediately:

```bash
# Get the proxy canister principal
icp network status --json | jq -r .proxy_canister_principal

# Forward a call with cycles
icp canister call my-canister method '(args)' \
  --proxy $(icp network status --json | jq -r .proxy_canister_principal) \
  --cycles 500_000_000_000
```

You do not need to manage this proxy — icp-cli handles its lifecycle.

## Connected Networks: Deploy Your Own Proxy

On connected networks (`ic` mainnet and custom networks), no proxy is provided. You must deploy one before you can forward cycles or reach canister-only methods.

### Using the Proxy Template

The fastest path is the `proxy` template:

```bash
# Create a new project from the proxy template
icp new my-proxy --subfolder proxy
cd my-proxy

# Deploy to IC mainnet
icp deploy -e ic

# Export the proxy canister ID
export PROXY_ID=$(icp canister status -e ic --id-only proxy)
```

The proxy canister starts with your deploying identity as its only controller.

### Funding the Proxy

The proxy must hold enough cycles to cover forwarded calls. Top it up via the cycles ledger:

```bash
# Transfer 5T cycles to the proxy
icp canister top-up $PROXY_ID --amount 5t -e ic
```

Check the proxy balance:

```bash
icp canister status $PROXY_ID -e ic
```

## Using `--proxy` and `--cycles`

Pass `--proxy <PRINCIPAL>` to any icp-cli command that needs to go through the proxy. Add `--cycles <AMOUNT>` when the target operation requires cycles.

### Canister Calls

```bash
# Call a method and attach 1T cycles
icp canister call my-canister charge_me '()' \
  -e ic \
  --proxy $PROXY_ID \
  --cycles 1_000_000_000_000

# Call a canister-only management method (no cycles needed)
icp canister status my-canister -e ic --proxy $PROXY_ID
```

### Canister Creation

```bash
# Create a new canister, funded with 3T cycles
icp canister create my-canister \
  -e ic \
  --proxy $PROXY_ID \
  --cycles 3_000_000_000_000
```

The new canister is created on the same subnet as the proxy.

### Deployment

When deploying to mainnet, pass `--proxy` so that icp-cli can create canisters and call management methods:

```bash
icp deploy -e ic --proxy $PROXY_ID
```

## Authorization

The proxy only accepts calls from its own **controllers**. Any call from a non-controller principal is rejected before it reaches the replicated state — protecting the proxy's cycles from unauthorized use.

After deploying the proxy, verify your identity is a controller:

```bash
dfx canister status $PROXY_ID --network ic
# or
icp canister status $PROXY_ID -e ic
```

To add another identity as a controller:

```bash
icp canister settings update $PROXY_ID --add-controller <PRINCIPAL> -e ic
```

## How the Proxy Works

The proxy canister exposes a single update method:

```candid
type ProxyArgs = record {
  canister_id : principal;
  method      : text;
  args        : blob;
  cycles      : nat;
};

service : {
  proxy : (ProxyArgs) -> (variant { Ok : record { result : blob }; Err : ... })
}
```

When icp-cli's `--proxy` flag is set:

1. The CLI Candid-encodes your original call arguments.
2. It wraps them into a `ProxyArgs` record alongside the target canister ID and `--cycles` amount.
3. It sends this as a single update call to the proxy's `proxy` method.
4. The proxy deducts the specified cycles from its own balance and forwards the call.
5. The response bytes are decoded and returned to you as if you had called the target directly.

For management canister calls, the CLI also sets an `effective_canister_id` on the request to ensure the IC routes it to the correct subnet.

## Migrating from the dfx Wallet

The dfx wallet served a similar purpose — it forwarded calls and funded canister creation — but was a more complex canister with an address book, event log, and custodian system. The proxy canister is the icp-cli equivalent: simpler, leaner, and controller-based.

If you have an existing dfx wallet with cycles that you want to reuse as a proxy, see [Replacing the dfx Wallet Canister](../migration/from-dfx.md#replacing-the-dfx-wallet-canister) in the migration guide.

## Keeping the Proxy Funded

The proxy pays cycles from its own balance for every forwarded call. Monitor the balance regularly and top it up before it runs out:

```bash
# Check balance
icp canister status $PROXY_ID -e ic

# Top up with 10T cycles
icp canister top-up $PROXY_ID --amount 10t -e ic
```

If the proxy runs out of cycles, forwarded calls will return `InsufficientCycles`. The proxy itself will not be deleted — canisters freeze before they are deleted, and you can always top up a frozen canister.

## Next Steps

- [Deploying to Mainnet](deploying-to-mainnet.md) — Full mainnet deployment workflow
- [Tokens and Cycles](tokens-and-cycles.md) — Managing ICP and cycles with icp-cli
