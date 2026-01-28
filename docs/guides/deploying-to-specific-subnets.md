# Deploying to Specific Subnets

The Internet Computer is composed of independent [subnets](https://learn.internetcomputer.org/hc/en-us/articles/34209955782420-Subnet-Creation) — each a blockchain that hosts canisters. By default, icp-cli selects a subnet automatically, but you can target specific subnets when needed.

## When to Use Specific Subnets

By default, `icp deploy` automatically selects a subnet for your canisters. You might want to target a specific subnet for:

- **Geographic requirements** — Data residency compliance (e.g., European subnets)
- **Replication** — Larger subnets offer higher security and fault tolerance
- **Colocation** — Keep related canisters on the same subnet for efficient inter-canister calls

## Default Subnet Selection

When you don't specify a subnet, icp-cli uses this logic:

1. If canisters already exist in the environment, new canisters are created on the same subnet as existing ones (keeps your project colocated)
2. If no canisters exist yet, a random subnet is selected from the available application subnets

This default behavior works well for most projects.

## Finding Subnet IDs

Use the [ICP Dashboard](https://dashboard.internetcomputer.org/subnets) to browse available subnets:

1. Browse the subnet list or filter by type (Application, Fiduciary, etc.) or node location
2. Click on a subnet to view details like node count, location, and current load
3. Copy the subnet principal (e.g., `pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeez-fez7a-iae`)

To find which subnet an existing canister is on, search for the canister ID on the [ICP Dashboard](https://dashboard.internetcomputer.org) — the canister details page shows its subnet.

## Deploying to a Specific Subnet

Use the `--subnet` flag with either `icp deploy` or `icp canister create`:

```bash
# Deploy all canisters to a specific subnet
icp deploy -e ic --subnet pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeez-fez7a-iae

# Deploy a specific canister to a subnet
icp deploy my-canister -e ic --subnet pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeez-fez7a-iae

# Create a canister on a specific subnet (without deploying code)
icp canister create my-canister -e ic --subnet pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeez-fez7a-iae
```

The `--subnet` flag only affects canister creation. If the canister already exists, it remains on its current subnet.

## Local Network Subnets

For local development, you can configure multiple subnets to test cross-subnet (Xnet) calls. See the [Configuration Reference](../reference/configuration.md) for available subnet types and setup.

## Troubleshooting

**"Subnet not found" or similar errors**

Verify the subnet ID is correct and the subnet accepts new canisters. Some subnets (like NNS/System subnets) don't allow arbitrary canister creation.

**Canister on wrong subnet**

The IC supports [canister migration](https://forum.dfinity.org/t/canister-migrations-available-now/63181) between subnets, but icp-cli does not yet support this feature. For now, you can delete and redeploy:

```bash
icp canister delete my-canister -e ic
icp deploy my-canister -e ic --subnet <correct-subnet>
```

Note: Deleting a canister permanently destroys its state. Canister migration support in icp-cli is planned.

## Next Steps

- [Deploying to Mainnet](deploying-to-mainnet.md) — Complete mainnet deployment guide
- [Managing Environments](managing-environments.md) — Configure different deployment targets
