# Telemetry

`icp` collects anonymous usage data to help the team understand how the tool is used, prioritize features, and identify issues. This page describes what is collected, how to opt out, and how the system works.

## What is collected

Each command invocation produces a single telemetry record with the following fields:

| Field | Example | Purpose |
|---|---|---|
| `batch` | `a1b2c3d4-...` | Group records from the same transmission; server-side deduplication |
| `sequence` | `0`, `1`, `2` | Ordering of records within a batch |
| `machine_id` | `a1b2c3d4-...` | Count unique installations |
| `platform` | `macos`, `linux`, `windows`, `wsl` | Platform distribution |
| `arch` | `aarch64`, `x86_64` | Architecture distribution |
| `version` | `0.1.0` | Identify version adoption |
| `date` | `2026-02-24` | UTC date of the event, for timeseries analysis |
| `command` | `build`, `deploy`, `canister status` | Feature usage |
| `arguments` | (see below) | Argument usage |
| `autocontainerize` | `true` / `false` (optional) | Track adoption of autocontainerize setting |
| `success` | `true` / `false` | Error rates |
| `duration_ms` | `4230` | Performance insights |
| `identity_type` | `pem-file`, `keyring` (optional) | Identity storage method distribution |
| `network_type` | `managed`, `connected` (optional) | Network target distribution |
| `num_canisters` | `3` (optional) | Number of canisters in the project |
| `recipes` | `["@dfinity/motoko@v4.0.0", "@dfinity/rust@v3.1.0"]` (optional) | Registry recipe distribution |

Each entry in `arguments` contains:

| Field | Description |
|---|---|
| `name` | The argument identifier (e.g. `mode`, `environment`) |
| `source` | How it was supplied: `command-line` or `environment` |
| `value` | The value, **only** if the argument has a constrained set of allowed values (e.g. `--mode install` where `mode` accepts `auto`, `install`, `reinstall`, `upgrade`). Free-form values (paths, principals, canister names, etc.) are always `null`. |

For example, `icp deploy --mode install --environment production` records:

```json
[
  {"name": "mode", "value": "install", "source": "command-line"},
  {"name": "environment", "value": null, "source": "command-line"}
]
```

The `batch` UUID is generated fresh each time records are transmitted and is not persisted across sends. Records within the same batch can be grouped for server-side deduplication.

The `machine_id` is a random UUID generated on first run and stored locally. It is used solely to count unique installations and is not linked to any user identity.

Additional fields may be introduced in future versions. This page will be updated accordingly. The same privacy principles apply: no personally identifiable information, no project data.

## What is not collected

- IP addresses, usernames, or any personally identifiable information
- Project names, file paths, or file contents
- Canister IDs, wallet addresses, or cycle balances
- Free-form argument values (only values from constrained `possible_values` sets are recorded)
- Error messages or stack traces

## Opting out

Any of the following disables telemetry:

| Method | Example |
|---|---|
| CLI setting | `icp settings telemetry false` |
| Environment variable | `ICP_TELEMETRY_DISABLED=1` |
| Cross-tool standard | `DO_NOT_TRACK=1` |
| CI environments | Automatically disabled when `CI` is set |

To re-enable: `icp settings telemetry true` (or unset the environment variable).

To check current status: `icp settings telemetry` (prints the current value).

Telemetry is **enabled by default**. On first run, a one-time notice is displayed:

```
icp collects anonymous usage data to improve the tool.
Run `icp settings telemetry false` or set DO_NOT_TRACK=1 to opt out.
Learn more: https://github.com/dfinity/icp-cli/blob/v<version>/docs/telemetry.md
```

## How data is stored and sent

Records are written as JSON lines to a local file you can inspect at any time:

| Platform | Path |
|---|---|
| macOS | `~/Library/Application Support/org.dfinity.icp-cli/telemetry/events.jsonl` |
| Linux | `~/.local/share/icp-cli/telemetry/events.jsonl` |
| Windows | `{FOLDERID_RoamingAppData}\dfinity\icp-cli\data\telemetry\events.jsonl` |

When `ICP_HOME` is set, telemetry data is stored under `$ICP_HOME/telemetry/` instead.

Records are sent in batches every few days (or sooner if the file grows large). Sending happens in a background process and never slows down the CLI. If a send fails, records are kept locally and retried later. Unsent records older than 14 days are automatically discarded.
