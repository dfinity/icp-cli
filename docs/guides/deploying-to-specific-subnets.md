# Deploying to Specific Subnets

The Internet Computer is composed of independent [subnets](https://internetcomputer.org/docs/concepts/subnets) — each a blockchain that hosts canisters. By default, icp-cli selects a subnet automatically, but you can target specific subnets for geographic, security, or capability requirements.

## When to Use Specific Subnets

By default, `icp deploy` automatically selects a subnet for your canisters. You might want to target a specific subnet when:

- **Verified Application Subnets** — Deploy to subnets with additional security guarantees
- **Geographic Requirements** — Target subnets in specific regions (e.g., European subnets for data residency)
- **Specialized Subnets** — Use subnets with specific capabilities (Bitcoin, Fiduciary, etc.)
- **Colocation** — Ensure related canisters are on the same subnet for efficient inter-canister calls

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

## Common Subnet Types

| Type | Description |
|------|-------------|
| Application | General-purpose subnets for most canisters |
| Verified Application | Subnets with additional security measures for high-value applications |
| Fiduciary | Handles sensitive operations like threshold ECDSA signatures |
| Bitcoin | Provides Bitcoin integration capabilities |
| System/NNS | Reserved for system canisters (not available for user deployment) |

The ICP Dashboard also allows filtering by node location (e.g., European subnets) for data residency requirements.

## Local Network Subnets

For local development, you can configure multiple subnets in `icp.yaml` to test cross-subnet (Xnet) calls:

```yaml
networks:
  - name: local
    mode: managed
    subnets:
      - application
      - application
```

Available local subnet types: `application`, `system`, `verified-application`, `bitcoin`, `fiduciary`, `nns`, `sns`

## Troubleshooting

**"Subnet not found" or similar errors**

Verify the subnet ID is correct and the subnet accepts new canisters. Some subnets (like NNS/System subnets) don't allow arbitrary canister creation.

**Canister on wrong subnet**

The IC supports [canister migration](https://internetcomputer.org/docs/building-apps/advanced-features/canister-migration) between subnets, but icp-cli does not yet support this feature. For now, you can delete and redeploy:

```bash
icp canister delete my-canister -e ic
icp deploy my-canister -e ic --subnet <correct-subnet>
```

Note: Deleting a canister permanently destroys its state. Canister migration support in icp-cli is planned.

## Next Steps

- [Deploying to Mainnet](deploying-to-mainnet.md) — Complete mainnet deployment guide
- [Managing Environments](managing-environments.md) — Configure different deployment targets
