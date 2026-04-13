# icp canister snapshot download

Download a snapshot to local disk

**Usage:** `icp canister snapshot download [OPTIONS] --output <OUTPUT> <CANISTER> <SNAPSHOT_ID>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified
* `<SNAPSHOT_ID>` — The snapshot ID to download (hex-encoded)

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `-o`, `--output <OUTPUT>` — Output directory for the snapshot files
* `--resume` — Resume a previously interrupted download
* `--proxy <PROXY>` — Principal of a proxy canister to route the management canister calls through




