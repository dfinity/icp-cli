# icp canister settings show

Show the settings of a canister.

Queries the canister_status endpoint of the management canister and displays only the settings fields. Requires the caller to be a controller.

**Usage:** `icp canister settings show [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--json` — Format output in json
* `--proxy <PROXY>` — Principal of a proxy canister to route the management canister call through




