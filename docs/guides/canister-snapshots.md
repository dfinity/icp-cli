# Canister Snapshots

Snapshots capture a canister's full state — WASM module, WASM memory, stable memory, and chunk store. Use them to back up canister state, transfer state between canisters, or recover from failed upgrades.

## When to Use Snapshots

- **Pre-upgrade backup** — Capture state before deploying a risky upgrade so you can roll back
- **State transfer** — Download a snapshot from one canister and upload it to another (required for [canister migration](canister-migration.md))
- **Disaster recovery** — Restore a canister to a known-good state
- **Offline inspection** — Download canister state to examine it locally

## Creating a Snapshot

Create a snapshot of a canister's current state. The canister must be stopped first:

```bash
icp canister stop my-canister -e ic
icp canister snapshot create my-canister -e ic
icp canister start my-canister -e ic
```

This returns a snapshot ID (hex string) that you'll use to reference this snapshot.

## Listing Snapshots

View all snapshots for a canister:

```bash
icp canister snapshot list my-canister -e ic
```

## Downloading a Snapshot

Download a snapshot to a local directory for backup or transfer:

```bash
icp canister snapshot download my-canister <snapshot-id> -o ./my-snapshot -e ic
```

The output directory will contain:

| File | Description |
|------|-------------|
| `metadata.json` | Snapshot metadata (timestamps, sizes, chunk hashes) |
| `wasm_module.bin` | The canister's WASM module |
| `wasm_memory.bin` | WASM heap memory |
| `stable_memory.bin` | Stable memory |
| `wasm_chunk_store/` | WASM chunk store files (one per chunk) |

For large canisters, downloads may take time. If interrupted, resume with:

```bash
icp canister snapshot download my-canister <snapshot-id> -o ./my-snapshot --resume -e ic
```

## Uploading a Snapshot

Upload a previously downloaded snapshot to a canister:

```bash
icp canister snapshot upload my-canister -i ./my-snapshot -e ic
```

This creates a new snapshot on the target canister from the local files.

To replace an existing snapshot instead of creating a new one:

```bash
icp canister snapshot upload my-canister -i ./my-snapshot --replace <snapshot-id> -e ic
```

Like downloads, interrupted uploads can be resumed:

```bash
icp canister snapshot upload my-canister -i ./my-snapshot --resume -e ic
```

## Restoring from a Snapshot

Restore a canister to the state captured in a snapshot. The canister must be stopped before restoring:

```bash
icp canister stop my-canister -e ic
icp canister snapshot restore my-canister <snapshot-id> -e ic
```

This replaces the canister's current WASM module, memory, and stable memory with the snapshot's contents. Start the canister again after restoring:

```bash
icp canister start my-canister -e ic
```

## Deleting Snapshots

Remove a snapshot you no longer need:

```bash
icp canister snapshot delete my-canister <snapshot-id> -e ic
```

## Example: Pre-Upgrade Backup

A common workflow is to create a snapshot before deploying an upgrade, so you can roll back if something goes wrong:

```bash
# 1. Stop the canister and create a snapshot before upgrading
icp canister stop my-canister -e ic
icp canister snapshot create my-canister -e ic
# Note the snapshot ID from the output

# 2. Deploy the upgrade
icp deploy my-canister -e ic

# 3. Test the upgrade
icp canister call my-canister health_check -e ic

# 4a. If everything works, optionally clean up the snapshot
icp canister snapshot delete my-canister <snapshot-id> -e ic

# 4b. If something is wrong, stop the canister and restore the snapshot
icp canister stop my-canister -e ic
icp canister snapshot restore my-canister <snapshot-id> -e ic
icp canister start my-canister -e ic
```

## Example: Transferring State Between Canisters

Download a snapshot from one canister and upload it to another. This workflow is essential for [canister migration](canister-migration.md), where you transfer state to a target canister on a different subnet before migrating the canister ID:

```bash
# Download from source
icp canister stop my-canister -e ic
icp canister snapshot create my-canister -e ic
icp canister start my-canister -e ic
icp canister snapshot download my-canister <snapshot-id> -o ./state-backup -e ic

# Upload to target (by canister ID if not in your project)
icp canister snapshot upload <target-id> -i ./state-backup -n ic
icp canister snapshot restore <target-id> <new-snapshot-id> -n ic
```

All snapshot commands accept either canister names (with `-e`) or canister IDs (with `-n`).

## Next Steps

- [Canister Migration](canister-migration.md) — Move canisters between subnets
- [Deploying to Mainnet](deploying-to-mainnet.md) — Production deployment guide

[Browse all documentation →](../index.md)
