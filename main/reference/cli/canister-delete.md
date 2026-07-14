# icp canister delete

Delete a canister from a network.

Cycles will be sent to the caller via the cycles ledger. This is done by installing a temporary shim canister.

**Usage:** `icp canister delete [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`. One of `mainnet`, `fetch`, or a 266-character hex-encoded root key
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--proxy <PROXY>` — Principal of a proxy canister to route the management canister call through
* `--no-recover-cycles` — Skip recovering the canister's liquid cycles to your cycles-ledger account before deletion (they are burned instead)




