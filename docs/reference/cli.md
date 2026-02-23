# Command-Line Help for `icp`

This document contains the help content for the `icp` command-line program.

**Command Overview:**

* [`icp`↴](#icp)
* [`icp build`↴](#icp-build)
* [`icp canister`↴](#icp-canister)
* [`icp canister call`↴](#icp-canister-call)
* [`icp canister create`↴](#icp-canister-create)
* [`icp canister delete`↴](#icp-canister-delete)
* [`icp canister install`↴](#icp-canister-install)
* [`icp canister list`↴](#icp-canister-list)
* [`icp canister logs`↴](#icp-canister-logs)
* [`icp canister metadata`↴](#icp-canister-metadata)
* [`icp canister migrate-id`↴](#icp-canister-migrate-id)
* [`icp canister settings`↴](#icp-canister-settings)
* [`icp canister settings show`↴](#icp-canister-settings-show)
* [`icp canister settings update`↴](#icp-canister-settings-update)
* [`icp canister settings sync`↴](#icp-canister-settings-sync)
* [`icp canister snapshot`↴](#icp-canister-snapshot)
* [`icp canister snapshot create`↴](#icp-canister-snapshot-create)
* [`icp canister snapshot delete`↴](#icp-canister-snapshot-delete)
* [`icp canister snapshot download`↴](#icp-canister-snapshot-download)
* [`icp canister snapshot list`↴](#icp-canister-snapshot-list)
* [`icp canister snapshot restore`↴](#icp-canister-snapshot-restore)
* [`icp canister snapshot upload`↴](#icp-canister-snapshot-upload)
* [`icp canister start`↴](#icp-canister-start)
* [`icp canister status`↴](#icp-canister-status)
* [`icp canister stop`↴](#icp-canister-stop)
* [`icp canister top-up`↴](#icp-canister-top-up)
* [`icp cycles`↴](#icp-cycles)
* [`icp cycles balance`↴](#icp-cycles-balance)
* [`icp cycles mint`↴](#icp-cycles-mint)
* [`icp cycles transfer`↴](#icp-cycles-transfer)
* [`icp deploy`↴](#icp-deploy)
* [`icp environment`↴](#icp-environment)
* [`icp environment list`↴](#icp-environment-list)
* [`icp identity`↴](#icp-identity)
* [`icp identity account-id`↴](#icp-identity-account-id)
* [`icp identity default`↴](#icp-identity-default)
* [`icp identity delete`↴](#icp-identity-delete)
* [`icp identity export`↴](#icp-identity-export)
* [`icp identity import`↴](#icp-identity-import)
* [`icp identity link`↴](#icp-identity-link)
* [`icp identity link hsm`↴](#icp-identity-link-hsm)
* [`icp identity list`↴](#icp-identity-list)
* [`icp identity new`↴](#icp-identity-new)
* [`icp identity principal`↴](#icp-identity-principal)
* [`icp identity rename`↴](#icp-identity-rename)
* [`icp network`↴](#icp-network)
* [`icp network list`↴](#icp-network-list)
* [`icp network ping`↴](#icp-network-ping)
* [`icp network start`↴](#icp-network-start)
* [`icp network status`↴](#icp-network-status)
* [`icp network stop`↴](#icp-network-stop)
* [`icp network update`↴](#icp-network-update)
* [`icp new`↴](#icp-new)
* [`icp project`↴](#icp-project)
* [`icp project show`↴](#icp-project-show)
* [`icp settings`↴](#icp-settings)
* [`icp settings autocontainerize`↴](#icp-settings-autocontainerize)
* [`icp sync`↴](#icp-sync)
* [`icp token`↴](#icp-token)
* [`icp token balance`↴](#icp-token-balance)
* [`icp token transfer`↴](#icp-token-transfer)

## `icp`

**Usage:** `icp [OPTIONS] [COMMAND]`

###### **Subcommands:**

* `build` — Build canisters
* `canister` — Perform canister operations against a network
* `cycles` — Mint and manage cycles
* `deploy` — Deploy a project to an environment
* `environment` — Show information about the current project environments
* `identity` — Manage your identities
* `network` — Launch and manage local test networks
* `new` — Create a new ICP project from a template
* `project` — Display information about the current project
* `settings` — Configure user settings
* `sync` — Synchronize canisters
* `token` — Perform token transactions

###### **Options:**

* `--project-root-override <PROJECT_ROOT_OVERRIDE>` — Directory to use as your project root directory. If not specified the directory structure is traversed up until an icp.yaml file is found
* `--debug` — Enable debug logging

  Default value: `false`
* `--identity-password-file <FILE>` — Read identity password from a file instead of prompting



## `icp build`

Build canisters

**Usage:** `icp build [OPTIONS] [CANISTERS]...`

###### **Arguments:**

* `<CANISTERS>` — Canister names (if empty, build all canisters in environment)

###### **Options:**

* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used



## `icp canister`

Perform canister operations against a network

**Usage:** `icp canister <COMMAND>`

###### **Subcommands:**

* `call` — Make a canister call
* `create` — Create a canister on a network
* `delete` — Delete a canister from a network
* `install` — Install a built WASM to a canister on a network
* `list` — List the canisters in an environment
* `logs` — Fetch and display canister logs
* `metadata` — Read a metadata section from a canister
* `migrate-id` — Migrate a canister ID from one subnet to another
* `settings` — Commands to manage canister settings
* `snapshot` — Commands to manage canister snapshots
* `start` — Start a canister on a network
* `status` — Show the status of canister(s)
* `stop` — Stop a canister on a network
* `top-up` — Top up a canister with cycles



## `icp canister call`

Make a canister call

**Usage:** `icp canister call [OPTIONS] <CANISTER> <METHOD> [ARGS]`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified
* `<METHOD>` — Name of canister method to call into
* `<ARGS>` — Call arguments, interpreted per `--args-format` (Candid by default). If not provided, an interactive prompt will be launched

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--args-file <ARGS_FILE>` — Path to a file containing call arguments
* `--args-format <ARGS_FORMAT>` — Format of the call arguments

  Default value: `candid`

  Possible values:
  - `hex`:
    Hex-encoded bytes
  - `candid`:
    Candid text format
  - `bin`:
    Raw binary (only valid for file references)

* `--proxy <PROXY>` — Principal of a proxy canister to route the call through.

   When specified, instead of calling the target canister directly, the call will be sent to the proxy canister's `proxy` method, which forwards it to the target canister.
* `--cycles <CYCLES>` — Cycles to forward with the proxied call.

   Only used when --proxy is specified. Defaults to 0.

  Default value: `0`
* `--query` — Sends a query request to a canister instead of an update request.

   Query calls are faster but return uncertified responses. Cannot be used with --proxy (proxy calls are always update calls).



## `icp canister create`

Create a canister on a network

**Usage:** `icp canister create [OPTIONS] [CANISTER]`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--controller <CONTROLLER>` — One or more controllers for the canister. Repeat `--controller` to specify multiple
* `--compute-allocation <COMPUTE_ALLOCATION>` — Optional compute allocation (0 to 100). Represents guaranteed compute capacity
* `--memory-allocation <MEMORY_ALLOCATION>` — Optional memory allocation in bytes. If unset, memory is allocated dynamically
* `--freezing-threshold <FREEZING_THRESHOLD>` — Optional freezing threshold in seconds. Controls how long a canister can be inactive before being frozen
* `--reserved-cycles-limit <RESERVED_CYCLES_LIMIT>` — Optional upper limit on cycles reserved for future resource payments. Memory allocations that would push the reserved balance above this limit will fail. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `-q`, `--quiet` — Suppress human-readable output; print only canister IDs, one per line, to stdout
* `--cycles <CYCLES>` — Cycles to fund canister creation. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)

  Default value: `2000000000000`
* `--subnet <SUBNET>` — The subnet to create canisters on
* `-i`, `--id-only` — Only print the canister id



## `icp canister delete`

Delete a canister from a network

**Usage:** `icp canister delete [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister install`

Install a built WASM to a canister on a network

**Usage:** `icp canister install [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-m`, `--mode <MODE>` — Specifies the mode of canister installation

  Default value: `auto`

  Possible values: `auto`, `install`, `reinstall`, `upgrade`

* `--wasm <WASM>` — Path to the WASM file to install. Uses the build output if not explicitly provided
* `--args <ARGS>` — Inline initialization arguments, interpreted per `--args-format` (Candid by default)
* `--args-file <ARGS_FILE>` — Path to a file containing initialization arguments
* `--args-format <ARGS_FORMAT>` — Format of the initialization arguments

  Default value: `candid`

  Possible values:
  - `hex`:
    Hex-encoded bytes
  - `candid`:
    Candid text format
  - `bin`:
    Raw binary (only valid for file references)

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister list`

List the canisters in an environment

**Usage:** `icp canister list [OPTIONS]`

###### **Options:**

* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used



## `icp canister logs`

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



## `icp canister metadata`

Read a metadata section from a canister

**Usage:** `icp canister metadata [OPTIONS] <CANISTER> <METADATA_NAME>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified
* `<METADATA_NAME>` — The name of the metadata section to read

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister migrate-id`

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



## `icp canister settings`

Commands to manage canister settings

**Usage:** `icp canister settings <COMMAND>`

###### **Subcommands:**

* `show` — Show the status of a canister
* `update` — Change a canister's settings to specified values
* `sync` — Synchronize a canister's settings with those defined in the project



## `icp canister settings show`

Show the status of a canister.

By default this queries the status endpoint of the management canister. If the caller is not a controller, falls back on fetching public information from the state tree.

**Usage:** `icp canister settings show [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — canister name or principal to target. When using a name, an enviroment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `-i`, `--id-only` — Only print the canister ids
* `--json` — Format output in json
* `-p`, `--public` — Show the only the public information. Skips trying to get the status from the management canister and looks up public information from the state tree



## `icp canister settings update`

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
* `--compute-allocation <COMPUTE_ALLOCATION>`
* `--memory-allocation <MEMORY_ALLOCATION>`
* `--freezing-threshold <FREEZING_THRESHOLD>`
* `--reserved-cycles-limit <RESERVED_CYCLES_LIMIT>` — Upper limit on cycles reserved for future resource payments. Memory allocations that would push the reserved balance above this limit will fail. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `--wasm-memory-limit <WASM_MEMORY_LIMIT>`
* `--wasm-memory-threshold <WASM_MEMORY_THRESHOLD>`
* `--log-visibility <LOG_VISIBILITY>`
* `--add-log-viewer <ADD_LOG_VIEWER>`
* `--remove-log-viewer <REMOVE_LOG_VIEWER>`
* `--set-log-viewer <SET_LOG_VIEWER>`
* `--add-environment-variable <ADD_ENVIRONMENT_VARIABLE>`
* `--remove-environment-variable <REMOVE_ENVIRONMENT_VARIABLE>`



## `icp canister settings sync`

Synchronize a canister's settings with those defined in the project

**Usage:** `icp canister settings sync [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister snapshot`

Commands to manage canister snapshots

**Usage:** `icp canister snapshot <COMMAND>`

###### **Subcommands:**

* `create` — Create a snapshot of a canister's state
* `delete` — Delete a canister snapshot
* `download` — Download a snapshot to local disk
* `list` — List all snapshots for a canister
* `restore` — Restore a canister from a snapshot
* `upload` — Upload a snapshot from local disk



## `icp canister snapshot create`

Create a snapshot of a canister's state

**Usage:** `icp canister snapshot create [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--replace <REPLACE>` — Replace an existing snapshot instead of creating a new one. The old snapshot will be deleted once the new one is successfully created



## `icp canister snapshot delete`

Delete a canister snapshot

**Usage:** `icp canister snapshot delete [OPTIONS] <CANISTER> <SNAPSHOT_ID>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified
* `<SNAPSHOT_ID>` — The snapshot ID to delete (hex-encoded)

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister snapshot download`

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



## `icp canister snapshot list`

List all snapshots for a canister

**Usage:** `icp canister snapshot list [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister snapshot restore`

Restore a canister from a snapshot

**Usage:** `icp canister snapshot restore [OPTIONS] <CANISTER> <SNAPSHOT_ID>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified
* `<SNAPSHOT_ID>` — The snapshot ID to restore (hex-encoded)

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister snapshot upload`

Upload a snapshot from local disk

**Usage:** `icp canister snapshot upload [OPTIONS] --input <INPUT> <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `-i`, `--input <INPUT>` — Input directory containing the snapshot files
* `--replace <REPLACE>` — Replace an existing snapshot instead of creating a new one
* `--resume` — Resume a previously interrupted upload



## `icp canister start`

Start a canister on a network

**Usage:** `icp canister start [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister status`

Show the status of canister(s).

By default this queries the status endpoint of the management canister. If the caller is not a controller, falls back on fetching public information from the state tree.

**Usage:** `icp canister status [OPTIONS] [CANISTER]`

###### **Arguments:**

* `<CANISTER>` — An optional canister name or principal to target. When using a name, an enviroment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `-i`, `--id-only` — Only print the canister ids
* `--json` — Format output in json
* `-p`, `--public` — Show the only the public information. Skips trying to get the status from the management canister and looks up public information from the state tree



## `icp canister stop`

Stop a canister on a network

**Usage:** `icp canister stop [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister top-up`

Top up a canister with cycles

**Usage:** `icp canister top-up [OPTIONS] --amount <AMOUNT> <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `--amount <AMOUNT>` — Amount of cycles to top up. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp cycles`

Mint and manage cycles

**Usage:** `icp cycles <COMMAND>`

###### **Subcommands:**

* `balance` — Display the cycles balance
* `mint` — Convert icp to cycles
* `transfer` — Transfer cycles to another principal



## `icp cycles balance`

Display the cycles balance

**Usage:** `icp cycles balance [OPTIONS]`

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--subaccount <SUBACCOUNT>` — The subaccount to check the balance for



## `icp cycles mint`

Convert icp to cycles

**Usage:** `icp cycles mint [OPTIONS]`

###### **Options:**

* `--icp <ICP>` — Amount of ICP to mint to cycles. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `--cycles <CYCLES>` — Amount of cycles to mint. Automatically determines the amount of ICP needed. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `--from-subaccount <FROM_SUBACCOUNT>` — Subaccount to withdraw the ICP from
* `--to-subaccount <TO_SUBACCOUNT>` — Subaccount to deposit the cycles to
* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp cycles transfer`

Transfer cycles to another principal

**Usage:** `icp cycles transfer [OPTIONS] <AMOUNT> <RECEIVER>`

###### **Arguments:**

* `<AMOUNT>` — Cycles amount to transfer. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `<RECEIVER>` — The receiver of the cycles transfer

###### **Options:**

* `--to-subaccount <TO_SUBACCOUNT>` — The subaccount to transfer to (only if the receiver is a principal)
* `--from-subaccount <FROM_SUBACCOUNT>`
* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp deploy`

Deploy a project to an environment

**Usage:** `icp deploy [OPTIONS] [NAMES]...`

###### **Arguments:**

* `<NAMES>` — Canister names

###### **Options:**

* `-m`, `--mode <MODE>` — Specifies the mode of canister installation

  Default value: `auto`

  Possible values: `auto`, `install`, `reinstall`, `upgrade`

* `--subnet <SUBNET>` — The subnet to use for the canisters being deployed
* `--controller <CONTROLLER>` — One or more controllers for the canisters being deployed. Repeat `--controller` to specify multiple
* `--cycles <CYCLES>` — Cycles to fund canister creation. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)

  Default value: `2000000000000`
* `--identity <IDENTITY>` — The user identity to run this command as
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used



## `icp environment`

Show information about the current project environments

**Usage:** `icp environment <COMMAND>`

###### **Subcommands:**

* `list` — Display a list of enviroments



## `icp environment list`

Display a list of enviroments

**Usage:** `icp environment list`



## `icp identity`

Manage your identities

**Usage:** `icp identity <COMMAND>`

###### **Subcommands:**

* `account-id` — Display the ICP ledger and ICRC-1 account identifiers for the current identity
* `default` — Display the currently selected identity
* `delete` — Delete an identity
* `export` — Print the PEM file for the identity
* `import` — Import a new identity
* `link` — Link an external key to a new identity
* `list` — List the identities
* `new` — Create a new identity
* `principal` — Display the principal for the current identity
* `rename` — Rename an identity



## `icp identity account-id`

Display the ICP ledger and ICRC-1 account identifiers for the current identity

**Usage:** `icp identity account-id [OPTIONS]`

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--of-principal <OF_PRINCIPAL>` — Convert this Principal instead of the current identity's Principal
* `--of-subaccount <OF_SUBACCOUNT>` — Specify a subaccount. If absent, the ICRC-1 account will be omitted as it is just the principal



## `icp identity default`

Display the currently selected identity

**Usage:** `icp identity default [NAME]`

###### **Arguments:**

* `<NAME>` — Identity to set as default. If omitted, prints the current default



## `icp identity delete`

Delete an identity

**Usage:** `icp identity delete <NAME>`

###### **Arguments:**

* `<NAME>` — Name of the identity to delete



## `icp identity export`

Print the PEM file for the identity

**Usage:** `icp identity export [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — Name of the identity to export

###### **Options:**

* `--password-file <FILE>` — Read the password from a file instead of prompting (only required for identities created or imported with --storage password)
* `--encrypt` — Encrypt the exported PEM with a password
* `--encryption-password-file <FILE>` — Read the encryption password from a file instead of prompting



## `icp identity import`

Import a new identity

**Usage:** `icp identity import [OPTIONS] <--from-pem <FILE>|--read-seed-phrase|--from-seed-file <FILE>> <NAME>`

###### **Arguments:**

* `<NAME>` — Name for the imported identity

###### **Options:**

* `--storage <STORAGE>` — Where to store the private key

  Default value: `keyring`

  Possible values: `plaintext`, `keyring`, `password`

* `--from-pem <FILE>` — Import from a PEM file
* `--read-seed-phrase` — Read seed phrase interactively from the terminal
* `--from-seed-file <FILE>` — Read seed phrase from a file
* `--decryption-password-from-file <FILE>` — Read the PEM decryption password from a file instead of prompting
* `--storage-password-file <FILE>` — Read the storage password from a file instead of prompting (for --storage password)
* `--assert-key-type <ASSERT_KEY_TYPE>` — Specify the key type when it cannot be detected from the PEM file (danger!)

  Possible values: `secp256k1`, `prime256v1`, `ed25519`




## `icp identity link`

Link an external key to a new identity

**Usage:** `icp identity link <COMMAND>`

###### **Subcommands:**

* `hsm` — Link an HSM key to a new identity



## `icp identity link hsm`

Link an HSM key to a new identity

**Usage:** `icp identity link hsm [OPTIONS] --pkcs11-module <PKCS11_MODULE> --key-id <KEY_ID> <NAME>`

###### **Arguments:**

* `<NAME>` — Name for the linked identity

###### **Options:**

* `--pkcs11-module <PKCS11_MODULE>` — Path to the PKCS#11 module (shared library) for the HSM
* `--slot <SLOT>` — Slot index on the HSM device

  Default value: `0`
* `--key-id <KEY_ID>` — Key ID on the HSM (e.g., "01" for PIV authentication key)
* `--pin-file <PIN_FILE>` — Read HSM PIN from a file instead of prompting



## `icp identity list`

List the identities

**Usage:** `icp identity list`



## `icp identity new`

Create a new identity

**Usage:** `icp identity new [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — Name for the new identity

###### **Options:**

* `--storage <STORAGE>` — Where to store the private key

  Default value: `keyring`

  Possible values: `plaintext`, `keyring`, `password`

* `--storage-password-file <FILE>` — Read the storage password from a file instead of prompting (for --storage password)
* `--output-seed <FILE>` — Write the seed phrase to a file instead of printing to stdout



## `icp identity principal`

Display the principal for the current identity

**Usage:** `icp identity principal [OPTIONS]`

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as



## `icp identity rename`

Rename an identity

**Usage:** `icp identity rename <OLD_NAME> <NEW_NAME>`

###### **Arguments:**

* `<OLD_NAME>` — Current name of the identity
* `<NEW_NAME>` — New name for the identity



## `icp network`

Launch and manage local test networks

**Usage:** `icp network <COMMAND>`

###### **Subcommands:**

* `list` — List all networks configured in the project
* `ping` — Try to connect to a network, and print out its status
* `start` — Run a given network
* `status` — Get status information about a running network
* `stop` — Stop a background network
* `update` — Update icp-cli-network-launcher to the latest version



## `icp network list`

List all networks configured in the project

**Usage:** `icp network list`



## `icp network ping`

Try to connect to a network, and print out its status

**Usage:** `icp network ping [OPTIONS] [NAME]`

Examples:

    # Ping default 'local' network
    icp network ping
  
    # Ping explicit network
    icp network ping mynetwork
  
    # Ping using environment flag
    icp network ping -e staging
  
    # Ping using ICP_ENVIRONMENT variable
    ICP_ENVIRONMENT=staging icp network ping
  
    # Name overrides ICP_ENVIRONMENT
    ICP_ENVIRONMENT=staging icp network ping local
  
    # Wait until healthy
    icp network ping --wait-healthy


###### **Arguments:**

* `<NAME>` — Name of the network to use.

   Takes precedence over -e/--environment and the ICP_ENVIRONMENT environment variable when specified explicitly.

###### **Options:**

* `-e`, `--environment <ENVIRONMENT>` — Use the network configured in the specified environment.

   Cannot be used together with an explicit network name argument.
   The ICP_ENVIRONMENT environment variable is also checked when neither network name nor -e flag is specified.
* `--wait-healthy` — Repeatedly ping until the replica is healthy or 1 minute has passed



## `icp network start`

Run a given network

**Usage:** `icp network start [OPTIONS] [NAME]`

Examples:

    # Use default 'local' network
    icp network start
  
    # Use explicit network name
    icp network start mynetwork
  
    # Use environment flag
    icp network start -e staging
  
    # Use ICP_ENVIRONMENT variable
    ICP_ENVIRONMENT=staging icp network start
  
    # Name overrides ICP_ENVIRONMENT
    ICP_ENVIRONMENT=staging icp network start local
  
    # Background mode with environment
    icp network start -e staging -d


###### **Arguments:**

* `<NAME>` — Name of the network to use.

   Takes precedence over -e/--environment and the ICP_ENVIRONMENT environment variable when specified explicitly.

###### **Options:**

* `-e`, `--environment <ENVIRONMENT>` — Use the network configured in the specified environment.

   Cannot be used together with an explicit network name argument.
   The ICP_ENVIRONMENT environment variable is also checked when neither network name nor -e flag is specified.
* `-d`, `--background` — Starts the network in a background process. This command will exit once the network is running. To stop the network, use 'icp network stop'



## `icp network status`

Get status information about a running network

**Usage:** `icp network status [OPTIONS] [NAME]`

Examples:

    # Get status of default 'local' network
    icp network status
  
    # Get status of explicit network
    icp network status mynetwork
  
    # Get status using environment flag
    icp network status -e staging
  
    # Get status using ICP_ENVIRONMENT variable
    ICP_ENVIRONMENT=staging icp network status
  
    # Name overrides ICP_ENVIRONMENT
    ICP_ENVIRONMENT=staging icp network status local
  
    # JSON output
    icp network status --json


###### **Arguments:**

* `<NAME>` — Name of the network to use.

   Takes precedence over -e/--environment and the ICP_ENVIRONMENT environment variable when specified explicitly.

###### **Options:**

* `-e`, `--environment <ENVIRONMENT>` — Use the network configured in the specified environment.

   Cannot be used together with an explicit network name argument.
   The ICP_ENVIRONMENT environment variable is also checked when neither network name nor -e flag is specified.
* `--json` — Format output as JSON



## `icp network stop`

Stop a background network

**Usage:** `icp network stop [OPTIONS] [NAME]`

Examples:

    # Stop default 'local' network
    icp network stop
  
    # Stop explicit network
    icp network stop mynetwork
  
    # Stop using environment flag
    icp network stop -e staging
  
    # Stop using ICP_ENVIRONMENT variable
    ICP_ENVIRONMENT=staging icp network stop
  
    # Name overrides ICP_ENVIRONMENT
    ICP_ENVIRONMENT=staging icp network stop local


###### **Arguments:**

* `<NAME>` — Name of the network to use.

   Takes precedence over -e/--environment and the ICP_ENVIRONMENT environment variable when specified explicitly.

###### **Options:**

* `-e`, `--environment <ENVIRONMENT>` — Use the network configured in the specified environment.

   Cannot be used together with an explicit network name argument.
   The ICP_ENVIRONMENT environment variable is also checked when neither network name nor -e flag is specified.



## `icp network update`

Update icp-cli-network-launcher to the latest version

**Usage:** `icp network update`



## `icp new`

Create a new ICP project from a template

Under the hood templates are generated with `cargo-generate`. See the cargo-generate docs for a guide on how to write your own templates: https://docs.rs/cargo-generate/0.23.7/cargo_generate/

**Usage:** `icp new [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — Directory to create / project name; if the name isn't in kebab-case, it will be converted to kebab-case unless `--force` is given

###### **Options:**

* `--subfolder <SUBFOLDER>` — Specifies the subfolder within the template repository to be used as the actual template
* `-g`, `--git <GIT>` — Git repository to clone template from. Can be a URL (like `https://github.com/dfinity/icp-cli-project-template`), a path (relative or absolute)

  Default value: `https://github.com/dfinity/icp-cli-templates`
* `-b`, `--branch <BRANCH>` — Branch to use when installing from git
* `-t`, `--tag <TAG>` — Tag to use when installing from git
* `-r`, `--revision <REVISION>` — Git revision to use when installing from git (e.g. a commit hash)
* `-p`, `--path <PATH>` — Local path to copy the template from. Can not be specified together with --git
* `-f`, `--force` — Don't convert the project name to kebab-case before creating the directory. Note that `icp-cli` won't overwrite an existing directory, even if `--force` is given
* `-q`, `--quiet` — Opposite of verbose, suppresses errors & warning in output Conflicts with --debug, and requires the use of --continue-on-error
* `--continue-on-error` — Continue if errors in templates are encountered
* `-s`, `--silent` — If silent mode is set all variables will be extracted from the template_values_file. If a value is missing the project generation will fail
* `--vcs <VCS>` — Specify the VCS used to initialize the generated template
* `-i`, `--identity <IDENTITY>` — Use a different ssh identity
* `--gitconfig <GITCONFIG_FILE>` — Use a different gitconfig file, if omitted the usual $HOME/.gitconfig will be used
* `-d`, `--define <DEFINE>` — Define a value for use during template expansion. E.g `--define foo=bar`
* `--init` — Generate the template directly into the current dir. No subfolder will be created and no vcs is initialized
* `--destination <PATH>` — Generate the template directly at the given path
* `--force-git-init` — Will enforce a fresh git init on the generated project
* `-o`, `--overwrite` — Allow the template to overwrite existing files in the destination
* `--skip-submodules` — Skip downloading git submodules (if there are any)



## `icp project`

Display information about the current project

**Usage:** `icp project <COMMAND>`

###### **Subcommands:**

* `show` — Outputs the project's effective yaml configuration



## `icp project show`

Outputs the project's effective yaml configuration.

The effective yaml configuration includes:

- implicit networks

- implicit environments

- processed recipes

**Usage:** `icp project show`



## `icp settings`

Configure user settings

**Usage:** `icp settings [OPTIONS] <SETTING> [VALUE]`

###### **Subcommands:**

* `autocontainerize` — Use Docker for the network launcher even when native mode is requested



## `icp settings autocontainerize`

Use Docker for the network launcher even when native mode is requested

**Usage:** `icp settings autocontainerize [VALUE]`

###### **Arguments:**

* `<VALUE>` — Set to true or false. If omitted, prints the current value

  Possible values: `true`, `false`




## `icp sync`

Synchronize canisters

**Usage:** `icp sync [OPTIONS] [CANISTERS]...`

###### **Arguments:**

* `<CANISTERS>` — Canister names (if empty, sync all canisters in environment)

###### **Options:**

* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp token`

Perform token transactions

**Usage:** `icp token [TOKEN|LEDGER_ID] <COMMAND>`

###### **Subcommands:**

* `balance` — Display the token balance on the ledger (default token: icp)
* `transfer` — Transfer ICP or ICRC1 tokens through their ledger (default token: icp)

###### **Arguments:**

* `<TOKEN|LEDGER_ID>` — The token or ledger canister id to execute the operation on, defaults to `icp`

  Default value: `icp`



## `icp token balance`

Display the token balance on the ledger (default token: icp)

**Usage:** `icp token [TOKEN|LEDGER_ID] balance [OPTIONS]`

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--subaccount <SUBACCOUNT>` — The subaccount to check the balance for



## `icp token transfer`

Transfer ICP or ICRC1 tokens through their ledger (default token: icp)

**Usage:** `icp token [TOKEN|LEDGER_ID] transfer [OPTIONS] <AMOUNT> <RECEIVER>`

###### **Arguments:**

* `<AMOUNT>` — Token amount to transfer. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `<RECEIVER>` — The receiver of the token transfer. Can be a principal, an ICRC1 account ID, or an ICP ledger account ID (hex)

###### **Options:**

* `--to-subaccount <TO_SUBACCOUNT>` — The subaccount to transfer to (only if the receiver is a principal)
* `--from-subaccount <FROM_SUBACCOUNT>` — The subaccount to transfer from
* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>

