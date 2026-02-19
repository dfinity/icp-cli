# Canister Migration

Move a canister to a different subnet. Depending on your needs, you can preserve just the canister's state, or both its state and canister ID.

## When to Migrate

- **Wrong subnet** — A canister was deployed to an unintended subnet
- **Geographic requirements** — Moving to a subnet in a specific region for data residency
- **Replication needs** — Moving to a larger subnet for higher fault tolerance
- **Colocation** — Consolidating canisters onto the same subnet for efficient inter-canister calls

## Choosing Your Approach

| Approach | State | Canister ID | Source Canister | Complexity |
|----------|-------|-------------|-----------------|------------|
| **Snapshot transfer** | Preserved | New ID | Retained | Moderate |
| **Full migration** (snapshot transfer + ID migration) | Preserved | Preserved | Deleted | Advanced |

**Snapshot transfer** — When you can accept a new canister ID. Create a new canister on the desired subnet, transfer state via [snapshots](canister-snapshots.md), and switch over. See [Migrating Without Preserving the Canister ID](#migrating-without-preserving-the-canister-id) below.

**Full migration** — When the canister ID must be preserved. This applies when the canister ID is load-bearing:

- **Threshold signatures (tECDSA / tSchnorr):** The IC derives signing keys by cryptographically binding them to the calling canister's principal. A canister's derived keys — and any addresses or public keys derived from them — are permanently tied to its ID. Losing the ID means losing access to those keys and any assets they control, whether those are addresses on other blockchains (Bitcoin, Ethereum, etc.) or ICP principals controlled by the canister.
- **VetKeys:** VetKey derivation similarly includes the canister's principal. A new ID produces entirely different decryption keys, making previously encrypted data inaccessible.
- **External references:** Other canisters, frontends, or off-chain systems that reference the canister by ID would break. This includes Internet Identity — users who authenticated via a canister-ID-based domain (e.g., `<canister-id>.icp0.io`) will lose access to their sessions.

See [Migrating With the Canister ID](#migrating-with-the-canister-id) below for the full workflow.

## Migrating Without Preserving the Canister ID

If you don't need to keep the canister ID, you can move state to a new canister using snapshots. This avoids the complexity of ID migration — no NNS migration canister, no cycle burn on the source, no minimum cycle requirement.

### 1. Create a New Canister

Add a temporary canister entry to your `icp.yaml` with a placeholder build step. This is required for `icp canister create` but the build itself won't run — state is transferred via snapshots instead:

```yaml
canisters:
  # ...your existing canisters...
  - name: migration-target
    build:
      steps:
        - type: script
          command: "true"
```

Create it on the desired subnet:

```bash
icp canister create migration-target -e ic --subnet <target-subnet-id>
```

Note the canister ID from the output — you'll use it in subsequent steps.

### 2. Transfer State via Snapshots

```bash
# Stop and snapshot the source canister
icp canister stop my-canister -e ic
icp canister snapshot create my-canister -e ic

# Download the snapshot locally
icp canister snapshot download my-canister <snapshot-id> -o ./migration-snapshot -e ic

# Upload and restore on the new canister
icp canister snapshot upload <target-id> -i ./migration-snapshot -n ic
icp canister snapshot restore <target-id> <new-snapshot-id> -n ic
```

See [Canister Snapshots](canister-snapshots.md) for details on resuming interrupted transfers.

### 3. Copy Settings

Snapshots capture WASM module and memory, but **not** canister settings. Check your source canister's settings and apply any non-default values to the new canister:

```bash
icp canister status my-canister -e ic

# Example: copy non-default settings
icp canister settings update <target-id> \
  --compute-allocation 10 \
  --freezing-threshold 604800 \
  -n ic
```

Run `icp canister settings update --help` for a full list of available settings. Common ones include compute allocation, memory allocation, and freezing threshold.

### 4. Switch Over

Start the new canister:

```bash
icp canister start <target-id> -n ic
```

**The old canister still exists** on its original subnet (stopped since step 2) and can be repurposed or deleted. Manage it before updating the project mapping, while `my-canister` still refers to it:

```bash
# Delete it if no longer needed
icp canister delete my-canister -e ic
```

**Update your project** to use the new canister going forward. icp-cli stores canister IDs per environment in `.icp/data/mappings/<environment>.ids.json` (for connected networks like mainnet) or `.icp/cache/mappings/<environment>.ids.json` (for managed networks). Update the mapping so `my-canister` points to the new canister's ID:

```json
{
  "my-canister": "<target-id>"
}
```

Remove the `migration-target` entry from the mappings file and from your `icp.yaml`.

**Update external references** — any other canisters, frontends, or off-chain systems that reference the old canister ID need to be updated to the new ID.

## Migrating With the Canister ID

When you need to preserve the canister ID, the process adds an ID migration step after transferring state. This uses `icp canister migrate-id` to move the canister ID from the source to the target canister on the new subnet.

> **Important:** The `migrate-id` command only moves the canister ID — it does **not** transfer state, settings, or cycles. If you skip the preparation steps, your canister's WASM module, memory, and stable memory will be lost. Follow the full workflow below.

### How the ID Migration Works

Under the hood, `icp canister migrate-id` tells the NNS migration canister to:

1. Rename the **target** canister to have the **source** canister's ID
2. Update the IC routing table so the source canister ID now resolves to the target's subnet
3. **Delete the source canister** from its original subnet (all remaining cycles are burned)
4. Restore the source canister's original controllers on the target

After this process:

- **Source canister** — Permanently deleted. Its cycles are burned and its canister ID now lives on the target's subnet.
- **Target canister** — Continues to exist on the same subnet, but now under the source canister's ID. It retains its own state, cycles, and settings (except controllers, which are restored from the source).
- **Target canister's original ID** — Ceases to exist permanently.

Because the target canister's state is what survives, **you must transfer state via snapshots before running `migrate-id`**. You should also copy any non-default settings and ensure the target has sufficient cycles for ongoing operation.

### 1. Create a Target Canister

Add a temporary canister entry to your `icp.yaml` with a placeholder build step. This is required for `icp canister create` but the build itself won't run — state is transferred via snapshots instead:

```yaml
canisters:
  # ...your existing canisters...
  - name: migration-target
    build:
      steps:
        - type: script
          command: "true"
```

Create it on the desired subnet:

```bash
icp canister create migration-target -e ic --subnet <target-subnet-id>
```

Note the canister ID from the output — you'll use it in all subsequent steps.

Top up the target canister with enough cycles for ongoing operation, since the source canister's cycles will be burned during the ID migration:

```bash
icp canister top-up <target-id> --amount 5T -n ic
```

### 2. Transfer State via Snapshots

Stop the source canister and create a snapshot, then download it, upload it to the target, and restore it:

```bash
# Stop and snapshot the source canister
icp canister stop my-canister -e ic
icp canister snapshot create my-canister -e ic
# Note the snapshot ID from the output

# Download the snapshot locally
icp canister snapshot download my-canister <snapshot-id> -o ./migration-snapshot -e ic

# Upload the snapshot to the target canister
icp canister snapshot upload <target-id> -i ./migration-snapshot -n ic

# Restore the snapshot on the target canister (use the new snapshot ID from the upload output)
icp canister snapshot restore <target-id> <new-snapshot-id> -n ic
```

After restoring, the target canister has the same WASM module, memory, and stable memory as the source.

**Delete the snapshot on the target** — the ID migration requires the target to have no snapshots:

```bash
icp canister snapshot delete <target-id> <new-snapshot-id> -n ic
```

For large canisters, downloads and uploads may take time. If interrupted, resume with the `--resume` flag. See [Canister Snapshots](canister-snapshots.md) for details.

### 3. Copy Settings

Snapshots capture WASM module and memory, but **not** canister settings. Controllers are automatically restored from the source during the ID migration, but other settings need to be copied manually.

Check your source canister's current settings:

```bash
icp canister status my-canister -e ic
```

If any settings differ from the defaults, apply them to the target canister:

```bash
# Example: copy non-default settings to the target canister
icp canister settings update <target-id> \
  --compute-allocation 10 \
  --freezing-threshold 604800 \
  --wasm-memory-limit 2GiB \
  -n ic
```

Run `icp canister settings update --help` for a full list of available settings. Common ones include compute allocation, memory allocation, and freezing threshold. You do **not** need to copy controllers — those are restored automatically.

### 4. Stop the Target Canister

Both canisters must be stopped before the ID migration. The source canister is already stopped from step 2, so only the target needs stopping:

```bash
icp canister stop <target-id> -n ic
```

### 5. Migrate the Canister ID

Run the ID migration. The `--replace` flag accepts both canister names and canister IDs:

```bash
icp canister migrate-id my-canister --replace <target-id> -e ic
```

This command:

1. Validates that both canisters meet the prerequisites (different subnets, stopped, sufficient cycles, no snapshots on target)
2. Asks for confirmation (skip with `-y`)
3. Adds the NNS migration canister as a controller of both canisters
4. Initiates the migration through the NNS migration canister
5. Polls migration status until complete

> **Cycles warning:** The source canister requires a minimum cycle balance for migration. **All remaining cycles on the source canister are burned** when it is deleted — they are not transferred to the target. If your source canister has a large cycle balance, consider reducing it before migrating. The command will warn you if the balance is high enough to warrant attention.

### 6. Wait for Completion

The command automatically polls for status and displays progress. Migration typically completes within a few minutes, but the command will wait up to 12 minutes before timing out.

On success, the source canister's ID now lives on the target's subnet with the state you transferred earlier. The source canister on the original subnet is permanently deleted.

### 7. Start and Verify

Start the canister to resume operation:

```bash
icp canister start my-canister -e ic
```

Verify the canister is on the expected subnet by querying the NNS Registry canister:

```bash
icp canister call rwlgt-iiaaa-aaaaa-aaaaa-cai get_subnet_for_canister \
  '(record { "principal" = opt principal "<canister-id>" })' --query -n ic
```

### 8. Clean Up

**Clean up the temporary canister entry.** Remove `migration-target` from your `icp.yaml` and from the ID mappings file (`.icp/data/mappings/<environment>.ids.json`), since its original ID no longer exists. The source canister keeps its original ID, so `my-canister` in your `icp.yaml` and mappings remains valid.

**Remove the NNS migration canister as controller** if desired — it is added during the ID migration and not automatically removed:

```bash
# Check controllers
icp canister status my-canister -e ic

# Remove the NNS migration canister as controller
icp canister settings update my-canister --remove-controller sbzkb-zqaaa-aaaaa-aaaiq-cai -e ic
```

**Delete local snapshot files** — remove the `./migration-snapshot` directory once you've verified the migration succeeded.

### Handling Interruptions

If the `migrate-id` command is interrupted or times out, the ID migration continues on the network. Use these flags to manage it:

**Resume watching:**

```bash
icp canister migrate-id my-canister --replace <target-id> --resume-watch -e ic
```

This skips validation and initiation, and resumes polling the migration status.

**Exit early:**

```bash
icp canister migrate-id my-canister --replace <target-id> --skip-watch -e ic
```

This exits early once the migration reaches an intermediate state, without waiting for full completion. Use `--resume-watch` later to verify the migration finished successfully.

## Troubleshooting

**"Canister is not ready for migration"**

The canister hasn't finished preparing for migration. Wait a few seconds and try again.

**"Canisters are on the same subnet"**

Migration requires canisters on different subnets. Create a canister on the desired subnet to use as the migration target:

```bash
icp canister create migration-target -e ic --subnet <target-subnet-id>
```

**"Target canister has snapshots"**

Delete all snapshots on the target canister first:

```bash
icp canister snapshot list <target-id> -n ic
icp canister snapshot delete <target-id> <snapshot-id> -n ic
```

**Insufficient cycles**

Top up the source canister to meet the minimum balance required for migration:

```bash
icp canister top-up my-canister --amount 1T -e ic
```

**Migration timed out**

The 12-minute timeout doesn't cancel the migration. Rerun with `--resume-watch` to continue watching:

```bash
icp canister migrate-id my-canister --replace <target-id> --resume-watch -e ic
```

## Next Steps

- [Canister Snapshots](canister-snapshots.md) — Full snapshot reference (download, upload, restore)
- [Deploying to Specific Subnets](deploying-to-specific-subnets.md) — Choose which subnet to deploy to

[Browse all documentation →](../index.md)
