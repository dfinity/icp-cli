# icp canister snapshot list

List all snapshots for a canister

**Usage:** `icp canister snapshot list [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--json` — Output command results as JSON
* `-q`, `--quiet` — Suppress human-readable output; print only snapshot IDs
* `--proxy <PROXY>` — Principal of a proxy canister to route the management canister call through




