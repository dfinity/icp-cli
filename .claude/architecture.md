# Architecture Details

## Project Model

The project model is built hierarchically through manifest consolidation:

1. **Project Manifest** (`icp.yaml`): Root configuration defining canisters, networks, and environments
2. **Canister Manifest** (`canister.yaml`): Per-canister configuration for build and sync steps
3. **Consolidated Project**: Final `Project` struct combining all manifests into a unified view

Key types in `crates/icp/src/lib.rs`:
- `Project`: Contains all canisters, networks, and environments
- `Environment`: Links a network with a set of canisters
- `Network`: Configuration for local (managed) or remote (connected) networks
- `Canister`: Build and sync configuration for a single canister

## Manifest System

Manifests are YAML files that define project structure. The system supports:

- **Inline definitions**: Define resources directly in `icp.yaml`
- **Path references**: Reference external manifest files
- **Glob patterns**: For canisters, use globs like `canisters/*` to auto-discover

The `consolidate_manifest` function in `crates/icp/src/project.rs` transforms raw manifests into the final `Project` structure. The serde structs in the `icp::manifest` module represent the format that the user's YAML files can be written in, while the serde structs with identical meaning outside `icp::manifest` are instead the canonical form, with defaults filled in and normalizations applied. Code should always deal with the canonical form.

## Build Adapters

Canisters are built using adapter pipelines defined in `crates/icp/src/manifest/adapter/`:

- **Script Adapter**: Runs shell commands with environment variables (e.g., `$ICP_WASM_OUTPUT_PATH`)
- **Prebuilt Adapter**: Uses pre-compiled WASM from local files, URLs, or registry
- **Assets Adapter**: Packages static assets for frontend canisters

Build steps are executed sequentially in `crates/icp/src/canister/build/`.

## Recipe System

Recipes are Handlebars templates that generate build/sync configuration. Implementation in `crates/icp/src/canister/recipe/`:

- **Registry recipes**: `@dfinity/rust@v3.0.0` resolves to GitHub releases URL
- **Local recipes**: `file://path/to/recipe.hbs`
- **Remote recipes**: Direct URLs with SHA256 verification

The `@dfinity` prefix is hardcoded to `https://github.com/dfinity/icp-cli-recipes/releases/download/{recipe}-{version}/recipe.hbs`

## Network Management

Two network types in `crates/icp/src/network/`:

- **Managed Networks**: Local test networks launched via `icp-cli-network-launcher` (wraps PocketIC)
- **Connected Networks**: Remote networks (mainnet, testnets) accessed via HTTP

### Implicit Networks and Environments

The CLI provides two implicit networks and environments that are always available:

- **`local` network**: A default managed network on `localhost:8000`. Users can override this in their `icp.yaml` to customize the local development environment (e.g., different port or connecting to an existing network).
- **`ic` network**: The IC mainnet at `https://icp-api.io`. This network is **protected** and cannot be overridden to prevent accidental production deployment with incorrect settings.

Corresponding implicit environments are also provided:
- **`local` environment**: Uses the `local` network with all project canisters. This is the default environment when none is specified.
- **`ic` environment**: Uses the `ic` network with all project canisters.

These constants are defined in `crates/icp/src/prelude.rs` as `LOCAL` and `IC` and are used throughout the codebase.

## Identity & Canister IDs

- **Identities**: Stored in platform-specific directories as PEM files (Secp256k1 or Ed25519):
  - macOS: `~/Library/Application Support/org.dfinity.icp-cli/identity/`
  - Linux: `~/.local/share/icp-cli/identity/`
  - Windows: `%APPDATA%\icp-cli\data\identity\`
  - Override with `ICP_HOME` environment variable: `$ICP_HOME/identity/`
- **Canister IDs**: Persisted in `.icp/{cache,data}/mappings/<environment>.ids.json` within project directories
  - Managed networks (local) use `.icp/cache/mappings/`
  - Connected networks (mainnet) use `.icp/data/mappings/`

Store management is in `crates/icp/src/store_id.rs`.

## Progress & User-Facing Output

Operations in `crates/icp-cli/src/operations/` report progress as data, not as terminal
calls. `crates/icp-events` defines the vocabulary (`Event`, `Reporter`, `Task`,
`OutputWriter`, `EventSink`, `CancelToken`) and depends only on serde and futures — never on
`icp`, an async runtime, or anything terminal-shaped. `crates/icp-cli/src/events.rs` holds
`IndicatifSink`, the only place that maps events onto `indicatif` bars, and the styles they
are drawn in.

- Operations take a `&Reporter`, never a `debug: bool`. Callers build one per operation with
  `events::indicatif_reporter(ctx.debug)`. The multi-canister operations
  (`build_many`, `sync_many`, `create_bundle`) still take one bool, `all_step_output`: it
  decides how much of a failure is replayed, not how anything is drawn.
- Nothing outside `events.rs` imports `indicatif`, with two exceptions that never went
  through the shared renderer and build their own one-off spinners:
  `commands/canister/migrate_id.rs` and `commands/identity/link/web.rs`. Everything else
  reports events. To check that this still holds:

  ```bash
  grep -rl 'use indicatif' crates/icp-cli/src crates/icp/src
  ```

- A library that produces output lines — a subprocess, a sync plugin — is handed an
  `OutputWriter` rather than a channel. Each line becomes an `Event::StepOutput` and is kept
  in the task's step log, capped at `MAX_RECORDED_LINES_PER_STEP`, so an operation can replay
  the failing step after the bars are down. `operations/step_replay.rs` formats that replay;
  read the steps back with `Task::recorded_steps` *before* finishing the task, since
  finishing consumes it.
- The event model is deliberately not semver-stable: `publish = false`, `0.x`, all enums
  `#[non_exhaustive]`, `TaskKind` closed.
- Events do not drive `--json`. `--json` means the command's final result; progress never
  appears in it.
- `tracing` at INFO level is product output here, not logging — `logging.rs` installs a
  `UserLayer` that prints `Level::INFO` to stderr unprefixed. `Event::Notice` is the event
  model's equivalent; the `info!`/`warn!`/`error!` calls inside `operations/` have not been
  converted yet.
- Under `--debug` the bars are hidden, so the `debug!` line the sink logs for each
  `Event::StepOutput` is the only thing tying that line to the canister that printed it —
  and canisters build in parallel, interleaving their output. `BarState` therefore keeps the
  prefix it gave the bar, and the log line reuses it, so both paths name a canister the same
  way. Anything else that has to name a task should read that prefix rather than the bar.
- A bar has to be fully styled and labelled before it is shown, and a spinner before
  `enable_steady_tick`: that call spawns a thread which draws immediately, so anything set
  afterwards races the first frame.

Operations are unit-tested by running them against `RecordingSink` and asserting on the
resulting `Vec<Event>`; see `operations/test_support.rs`. `events.rs::rendering`
additionally pins the frames `IndicatifSink` draws against the literal output of the
renderer it replaced, captured before that renderer was deleted.

## Telemetry

Anonymous usage telemetry implementation. User-facing documentation is in `docs/telemetry.md`.

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
  batch-<timestamp>-<uuid>.jsonl      # in-flight/pending batch(es)
```

### Transmission

A send is triggered at the end of a command if **either** threshold is met:

- **Time-based**: A randomized interval has elapsed since the last send (2–4 days for stable releases, 0.75–1.25 days for pre-release). The jitter prevents thundering herd problems.
- **Size-based**: The log file exceeds 256 KB. This acts as a safety valve against unbounded growth from rapid command usage.

When a send is triggered:

1. The log file is renamed to `telemetry/batch-<timestamp>-<uuid>.jsonl` (atomically moves it out of the write path). The UUID identifies the batch for server-side deduplication.
2. A detached background process is spawned. It injects the batch UUID and a per-record sequence number into each JSON line, then POSTs the payload. This ensures sending never blocks the CLI.
3. On success, the batch file is deleted and the next send time is randomized.
4. On failure, the batch file remains for retry on the next trigger. Since the batch UUID is stable in the filename, retried sends use the same ID, allowing the server to deduplicate.

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
rename events.jsonl to batch-<timestamp>-<uuid>.jsonl
delete stale batches (batch-*.jsonl >14 days old or >10 files)
spawn detached background process:
  inject batch UUID + sequence numbers into each JSON record
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
