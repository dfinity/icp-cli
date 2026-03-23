# icp canister migrate-id

Migrate a canister ID from one subnet to another

**Usage:** `icp canister migrate-id [OPTIONS] --replace <REPLACE> <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--replace <REPLACE>` — The canister to replace with the source canister's ID
* `-y`, `--yes` — Skip confirmation prompts
* `--resume-watch` — Resume watching an already-initiated migration (skips validation and initiation)
* `--skip-watch` — Exit as soon as the migrated canister is deleted (don't wait for full completion)




