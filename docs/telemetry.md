# Telemetry

`icp` collects anonymous usage data to help the team understand how the tool is used, prioritize features, and identify issues. This page describes what is collected, how to opt out, and how the system works.

## What is collected

Each command invocation produces a single telemetry record with the following fields:

| Field | Example | Purpose |
|---|---|---|
| `version` | `0.1.0` | Identify version adoption |
| `os` | `macos`, `linux`, `windows` | Platform distribution |
| `arch` | `aarch64`, `x86_64` | Architecture distribution |
| `command` | `build`, `deploy`, `canister status` | Feature usage |
| `arguments` | (see below) | Argument usage |
| `success` | `true` / `false` | Error rates |
| `duration_ms` | `4230` | Performance insights |
| `machine_id` | `a1b2c3d4-...` | Count unique installations |
| `timestamp` | `2026-02-23T12:00:00Z` | Usage trends |

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
Learn more: https://docs.icp-cli.dev/telemetry
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

---

## Implementation design

The rest of this page is intended for developers working on the telemetry system.

### Settings

The telemetry enabled/disabled flag is a user setting stored in the existing `Settings` struct (alongside `autocontainerize`), persisted at the standard settings path. It is controlled via `icp settings telemetry [true|false]`.

### Telemetry data directory

Runtime state and event data live in the `telemetry/` data directory. Each piece of state is a separate plain file to avoid JSON parsing:

```
telemetry/
  machine-id                          # plain text UUID, generated on first run
  notice-shown                        # empty marker file, presence = notice was shown
  next-send-time                      # plain text UTC timestamp
  events.jsonl                        # active event log
  events-sending-<timestamp>.jsonl    # in-flight/pending batch(es)
```

### Transmission

A send is triggered at the end of a command if **either** threshold is met:

- **Time-based**: A randomized interval has elapsed since the last send (2–4 days for stable releases, 0.75–1.25 days for pre-release). The jitter prevents thundering herd problems.
- **Size-based**: The log file exceeds 256 KB. This acts as a safety valve against unbounded growth from rapid command usage.

When a send is triggered:

1. The log file is renamed to `telemetry/events-sending-<timestamp>.jsonl` (atomically moves it out of the write path).
2. A detached background process is spawned to POST the batch. This ensures sending never blocks the CLI.
3. On success, the batch file is deleted and the next send time is randomized.
4. On failure, the batch file remains for retry on the next trigger.

Safeguards:

- **Concurrent send guard**: During an active send, the next send time is temporarily set 30 minutes in the future to prevent races.
- **Stale batch cleanup**: Batch files older than 14 days or exceeding 10 in count are deleted without sending. This prevents unbounded accumulation from repeated network failures.
- **Send timeout**: Each HTTP POST uses a 5-second timeout. The detached process exits silently on failure.

### Control flow

```
command start
  |
  v
check env vars (DO_NOT_TRACK, ICP_TELEMETRY_DISABLED, CI)
  |-- any set? --> skip telemetry entirely
  |
  v
load settings
  |-- disabled in settings? --> skip
  |
  v
check telemetry/notice-shown
  |-- missing? --> print notice, create marker file
  |
  v
execute command, measure duration
  |
  v
append record to events.jsonl
  |
  v
check send triggers:
  - next-send-time has passed? OR
  - events.jsonl > 256 KB?
  |-- neither met? --> done
  |
  v
set next-send-time to now + 30 min (concurrent send guard)
rename events.jsonl to events-sending-<timestamp>.jsonl
delete stale batches (events-sending-*.jsonl >14 days old or >10 files)
spawn detached background process:
  POST batch to server (5s timeout)
  on success: delete batch file
  on failure: leave batch file for next retry
  randomize next-send-time (2-4 days or 0.75-1.25 days for pre-release)
```

### Schema evolution

New fields can be added to the record struct without migration. In the Rust struct, new fields should use `Option<T>` or `#[serde(default)]` so that older records still in `events.jsonl` deserialize correctly. No versioning scheme is needed.

### Key principles

1. **Never block the CLI.** Telemetry checks and writes are fast. Network sends happen in a detached background process.
2. **Fail silently.** Any telemetry error (file I/O, network) is swallowed. Telemetry must never cause a command to fail.
3. **Full transparency.** Users can inspect the local file, check status, and opt out at any time.
4. **Minimal data.** Collect only what is listed above. No free-form argument values, no PII, no project data.
