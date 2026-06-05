# icp canister settings update

Change a canister's settings to specified values

**Usage:** `icp canister settings update [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `-f`, `--force` — Force the operation without confirmation prompts
* `--add-controller <ADD_CONTROLLER>` — Add one or more principals to the canister's controller list
* `--remove-controller <REMOVE_CONTROLLER>` — Remove one or more principals from the canister's controller list.

   Warning: Removing yourself will cause you to lose control of the canister.
* `--set-controller <SET_CONTROLLER>` — Replace the canister's controller list with the specified principals.

   Warning: This removes all existing controllers not in the new list. If you don't include yourself, you will lose control of the canister.
* `--compute-allocation <COMPUTE_ALLOCATION>` — Compute allocation percentage (0-100). Represents a guaranteed share of a subnet's compute capacity
* `--memory-allocation <MEMORY_ALLOCATION>` — Memory allocation in bytes. Supports suffixes: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb")
* `--freezing-threshold <FREEZING_THRESHOLD>` — Freezing threshold. Controls how long a canister can be inactive before being frozen. Supports duration suffixes: s (seconds), m (minutes), h (hours), d (days), w (weeks). A bare number is treated as seconds
* `--reserved-cycles-limit <RESERVED_CYCLES_LIMIT>` — Upper limit on cycles reserved for future resource payments. Memory allocations that would push the reserved balance above this limit will fail. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `--wasm-memory-limit <WASM_MEMORY_LIMIT>` — Wasm memory limit in bytes. Supports suffixes: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb")
* `--wasm-memory-threshold <WASM_MEMORY_THRESHOLD>` — Wasm memory threshold in bytes. Supports suffixes: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb")
* `--log-memory-limit <LOG_MEMORY_LIMIT>` — Log memory limit in bytes (max 2 MiB). Oldest logs are purged when usage exceeds this value. Supports suffixes: kb, kib, mb, mib (e.g. "2mib" or "256kib"). Canister default is 4096 bytes
* `--log-visibility <LOG_VISIBILITY>` — Set log visibility to a fixed policy [possible values: controllers, public]. Conflicts with --add-log-viewer, --remove-log-viewer, and --set-log-viewer. Use --add-log-viewer / --set-log-viewer to grant access to specific principals instead
* `--add-log-viewer <ADD_LOG_VIEWER>` — Add a principal to the allowed log viewers list
* `--remove-log-viewer <REMOVE_LOG_VIEWER>` — Remove a principal from the allowed log viewers list
* `--set-log-viewer <SET_LOG_VIEWER>` — Replace the allowed log viewers list with the specified principals
* `--add-environment-variable <ADD_ENVIRONMENT_VARIABLE>` — Add a canister environment variable in KEY=VALUE format
* `--remove-environment-variable <REMOVE_ENVIRONMENT_VARIABLE>` — Remove a canister environment variable by key name
* `--proxy <PROXY>` — Principal of a proxy canister to route the management canister calls through




