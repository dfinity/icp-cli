# icp canister logs

Fetch and display canister logs

**Usage:** `icp canister logs [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `-f`, `--follow` — Continuously fetch and display new logs until interrupted with Ctrl+C
* `--interval <INTERVAL>` — Polling interval in seconds when following logs (requires --follow)

  Default value: `2`
* `--since <TIMESTAMP>` — Show logs at or after this timestamp (inclusive). Accepts nanoseconds since Unix epoch or RFC3339 (e.g. '2024-01-01T00:00:00Z'). Cannot be used with --follow
* `--until <TIMESTAMP>` — Show logs before this timestamp (exclusive). Accepts nanoseconds since Unix epoch or RFC3339 (e.g. '2024-01-01T00:00:00Z'). Cannot be used with --follow
* `--since-index <INDEX>` — Show logs at or after this log index (inclusive). Cannot be used with --follow
* `--until-index <INDEX>` — Show logs before this log index (exclusive). Cannot be used with --follow
* `--json` — Output command results as JSON




