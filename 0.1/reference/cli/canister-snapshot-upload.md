# icp canister snapshot upload

Upload a snapshot from local disk

**Usage:** `icp canister snapshot upload [OPTIONS] --input <INPUT> <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `-i`, `--input <INPUT>` — Input directory containing the snapshot files
* `--replace <REPLACE>` — Replace an existing snapshot instead of creating a new one
* `--resume` — Resume a previously interrupted upload




