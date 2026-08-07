# Upgrading from icp-cli 0.2

The built-in **`assets` sync step** (`type: assets`) is removed in releases after
0.2.x — starting with the 0.3 release candidate and carried into 1.0.0. If you're
upgrading a project from icp-cli 0.2.x, this guide shows how to switch to the
plugin-based replacement.

## What changed

The `assets` sync step uploaded a directory to an asset canister from inside
icp-cli core. That capability is retired: asset uploading now lives outside the
CLI, in a recipe-provided [sync plugin](../concepts/sync-plugins.md). The
`script` and `plugin` sync steps are unaffected.

## Do you need to migrate?

You're affected if your `icp.yaml` (or a per-canister `canister.yaml`) either:

- contains a `type: assets` sync step, or
- uses the `@dfinity/asset-canister` recipe at a version below `v2.2.1` (those
  versions emit a `type: assets` step internally).

After upgrading, any command that loads such a manifest (`icp deploy`,
`icp sync`, …) fails to load it with a targeted error:

```
icp-cli no longer supports the `assets` sync step type. Switch to a `script` or
`plugin` sync step. If this step comes from a recipe, check whether a newer
version of the recipe uses a plugin-based solution.
```

Pick the path that matches how your project uploads assets:

- [Recipe users](#recipe-users) — bump the `@dfinity/asset-canister` recipe version.
- [Manual sync steps](#manual-sync-steps) — switch a hand-written `type: assets`
  step to the certified-assets sync plugin.

## Recipe users

If you reference the `@dfinity/asset-canister` recipe, upgrade it to `v2.2.1`.
That version emits a `plugin` sync step instead of the retired `assets` step;
nothing else in your configuration changes.

**Before:**

```yaml
canisters:
  - name: frontend
    recipe:
      type: "@dfinity/asset-canister@v2.1.0"
      configuration:
        dir: www
```

**After:**

```yaml
canisters:
  - name: frontend
    recipe:
      type: "@dfinity/asset-canister@v2.2.1"
      configuration:
        dir: www
```

The recipe's `configuration` (including `dir`) is unchanged. See the
[asset-canister v2.2.1 release](https://github.com/dfinity/icp-cli-recipes/releases/tag/asset-canister-v2.2.1)
for details.

## Manual sync steps

If your manifest declares `type: assets` directly, replace it with a `plugin`
sync step that points at the certified-assets migration plugin.

**Before:**

```yaml
sync:
  steps:
    - type: assets
      dirs:
        - www
```

**After:**

```yaml
sync:
  steps:
    - type: plugin
      url: https://github.com/dfinity/certified-assets/releases/download/migration-v2.2.1-6b48585/sync_plugin.wasm
      sha256: ca7cb5666c30d2875f8d5e10535f8a53f97a86c79c263f7d5bdac2fdd1bbf83c
      dirs:
        - www
```

The plugin is published in the
[certified-assets migration-v2.2.1 release](https://github.com/dfinity/certified-assets/releases/tag/migration-v2.2.1-6b48585).
It uploads the contents of a single directory to the asset canister being synced —
the same job the old `assets` step did. `dirs` is the general
[sync-plugin](../concepts/sync-plugins.md) field (a list, since a plugin may
declare several directories), but this particular plugin reads **exactly one** —
list a single entry. The `url`/`sha256` pin the exact wasm: icp-cli downloads it
once, verifies the checksum, and caches it.

Your **build step is unchanged** — keep building or providing the asset-canister
wasm exactly as before. Only the sync step changes.

### Convert `dir:` to `dirs:`

The `assets` step accepted either a single `dir:` or a list `dirs:`. The `plugin`
step only takes the list form, and this plugin reads exactly one directory from
it. If you used the singular `dir:`, wrap it in a single-element `dirs:` list:

**Before:**

```yaml
- type: assets
  dir: dist
```

**After:**

```yaml
- type: plugin
  url: https://github.com/dfinity/certified-assets/releases/download/migration-v2.2.1-6b48585/sync_plugin.wasm
  sha256: ca7cb5666c30d2875f8d5e10535f8a53f97a86c79c263f7d5bdac2fdd1bbf83c
  dirs:
    - dist
```

If your old `assets` step listed **more than one** directory, consolidate the
files into a single directory before uploading — this plugin accepts only one.

## Verify

After editing the manifest, redeploy:

```bash
icp deploy
```

The manifest now loads, the plugin downloads and its checksum is verified, and
your assets upload exactly as they did with the built-in step.

## See also

- [Sync Plugins](../concepts/sync-plugins.md) — how the plugin sandbox works
- [Plugin Sync (Configuration Reference)](../reference/configuration.md#plugin-sync) — the `plugin` step manifest fields
- [Using Recipes](../guides/using-recipes.md) — referencing and pinning recipe versions

[Browse all documentation →](../index.md)
