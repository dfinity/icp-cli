# icp canister settings update

Change a canister's settings to specified values

**Usage:** `icp canister settings update [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `-f`, `--force` — Force the operation without confirmation prompts
* `--add-controller <ADD_CONTROLLER>` — Add one or more principals to the canister's controller list
* `--remove-controller <REMOVE_CONTROLLER>` — Remove one or more principals from the canister's controller list.

   Warning: Removing yourself will cause you to lose control of the canister.
* `--set-controller <SET_CONTROLLER>` — Replace the canister's controller list with the specified principals.

   Warning: This removes all existing controllers not in the new list. If you don't include yourself, you will lose control of the canister.
* `--compute-allocation <COMPUTE_ALLOCATION>`
* `--memory-allocation <MEMORY_ALLOCATION>`
* `--freezing-threshold <FREEZING_THRESHOLD>`
* `--reserved-cycles-limit <RESERVED_CYCLES_LIMIT>` — Reserved cycles limit for the canister. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `--wasm-memory-limit <WASM_MEMORY_LIMIT>`
* `--wasm-memory-threshold <WASM_MEMORY_THRESHOLD>`
* `--log-visibility <LOG_VISIBILITY>`
* `--add-log-viewer <ADD_LOG_VIEWER>`
* `--remove-log-viewer <REMOVE_LOG_VIEWER>`
* `--set-log-viewer <SET_LOG_VIEWER>`
* `--add-environment-variable <ADD_ENVIRONMENT_VARIABLE>`
* `--remove-environment-variable <REMOVE_ENVIRONMENT_VARIABLE>`




