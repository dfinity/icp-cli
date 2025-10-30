# Command-Line Help for `icp-cli`

This document contains the help content for the `icp-cli` command-line program.

**Command Overview:**

* [`icp-cli`↴](#icp-cli)
* [`icp-cli build`↴](#icp-cli-build)
* [`icp-cli canister`↴](#icp-cli-canister)
* [`icp-cli canister call`↴](#icp-cli-canister-call)
* [`icp-cli canister create`↴](#icp-cli-canister-create)
* [`icp-cli canister delete`↴](#icp-cli-canister-delete)
* [`icp-cli canister info`↴](#icp-cli-canister-info)
* [`icp-cli canister install`↴](#icp-cli-canister-install)
* [`icp-cli canister list`↴](#icp-cli-canister-list)
* [`icp-cli canister settings`↴](#icp-cli-canister-settings)
* [`icp-cli canister settings show`↴](#icp-cli-canister-settings-show)
* [`icp-cli canister settings update`↴](#icp-cli-canister-settings-update)
* [`icp-cli canister show`↴](#icp-cli-canister-show)
* [`icp-cli canister snapshot`↴](#icp-cli-canister-snapshot)
* [`icp-cli canister snapshot create`↴](#icp-cli-canister-snapshot-create)
* [`icp-cli canister snapshot delete`↴](#icp-cli-canister-snapshot-delete)
* [`icp-cli canister snapshot download`↴](#icp-cli-canister-snapshot-download)
* [`icp-cli canister snapshot list`↴](#icp-cli-canister-snapshot-list)
* [`icp-cli canister snapshot load`↴](#icp-cli-canister-snapshot-load)
* [`icp-cli canister snapshot upload`↴](#icp-cli-canister-snapshot-upload)
* [`icp-cli canister start`↴](#icp-cli-canister-start)
* [`icp-cli canister status`↴](#icp-cli-canister-status)
* [`icp-cli canister stop`↴](#icp-cli-canister-stop)
* [`icp-cli canister top-up`↴](#icp-cli-canister-top-up)
* [`icp-cli cycles`↴](#icp-cli-cycles)
* [`icp-cli cycles balance`↴](#icp-cli-cycles-balance)
* [`icp-cli cycles mint`↴](#icp-cli-cycles-mint)
* [`icp-cli deploy`↴](#icp-cli-deploy)
* [`icp-cli environment`↴](#icp-cli-environment)
* [`icp-cli environment list`↴](#icp-cli-environment-list)
* [`icp-cli identity`↴](#icp-cli-identity)
* [`icp-cli identity default`↴](#icp-cli-identity-default)
* [`icp-cli identity import`↴](#icp-cli-identity-import)
* [`icp-cli identity list`↴](#icp-cli-identity-list)
* [`icp-cli identity new`↴](#icp-cli-identity-new)
* [`icp-cli identity principal`↴](#icp-cli-identity-principal)
* [`icp-cli network`↴](#icp-cli-network)
* [`icp-cli network list`↴](#icp-cli-network-list)
* [`icp-cli network ping`↴](#icp-cli-network-ping)
* [`icp-cli network run`↴](#icp-cli-network-run)
* [`icp-cli network stop`↴](#icp-cli-network-stop)
* [`icp-cli sync`↴](#icp-cli-sync)
* [`icp-cli token`↴](#icp-cli-token)
* [`icp-cli token balance`↴](#icp-cli-token-balance)
* [`icp-cli token transfer`↴](#icp-cli-token-transfer)

## `icp-cli`

**Usage:** `icp-cli [OPTIONS] [COMMAND]`

###### **Subcommands:**

* `build` — Build a project
* `canister` — Perform canister operations against a network
* `cycles` — Mint and manage cycles
* `deploy` — Deploy a project to an environment
* `environment` — Show information about the current project environments
* `identity` — Manage your identities
* `network` — Launch and manage local test networks
* `sync` — Synchronize canisters in the current environment
* `token` — Perform token transactions

###### **Options:**

* `--project-dir <PROJECT_DIR>` — Directory to use as your project base directory. If not specified the directory structure is traversed up until an icp.yaml file is found
* `--id-store <ID_STORE>`

  Default value: `.icp/ids.json`
* `--artifact-store <ARTIFACT_STORE>`

  Default value: `.icp/artifacts`
* `--debug` — Enable debug logging

  Default value: `false`



## `icp-cli build`

Build a project

**Usage:** `icp-cli build [NAMES]...`

###### **Arguments:**

* `<NAMES>` — The names of the canisters within the current project



## `icp-cli canister`

Perform canister operations against a network

**Usage:** `icp-cli canister <COMMAND>`

###### **Subcommands:**

* `call` — 
* `create` — 
* `delete` — 
* `info` — 
* `install` — 
* `list` — 
* `settings` — 
* `show` — 
* `snapshot` — 
* `start` — 
* `status` — 
* `stop` — 
* `top-up` — 



## `icp-cli canister call`

**Usage:** `icp-cli canister call [OPTIONS] <CANISTER> <METHOD> <ARGS>`

###### **Arguments:**

* `<CANISTER>` — Name of canister to target
* `<METHOD>` — Name of canister method to call into
* `<ARGS>` — String representation of canister call arguments

###### **Options:**

* `--network <NETWORK>`
* `--environment <ENVIRONMENT>`
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp-cli canister create`

**Usage:** `icp-cli canister create [OPTIONS] [NAMES]...`

###### **Arguments:**

* `<NAMES>` — The names of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic
* `--controller <CONTROLLER>` — One or more controllers for the canister. Repeat `--controller` to specify multiple
* `--compute-allocation <COMPUTE_ALLOCATION>` — Optional compute allocation (0 to 100). Represents guaranteed compute capacity
* `--memory-allocation <MEMORY_ALLOCATION>` — Optional memory allocation in bytes. If unset, memory is allocated dynamically
* `--freezing-threshold <FREEZING_THRESHOLD>` — Optional freezing threshold in seconds. Controls how long a canister can be inactive before being frozen
* `--reserved-cycles-limit <RESERVED_CYCLES_LIMIT>` — Optional reserved cycles limit. If set, the canister cannot consume more than this many cycles
* `-q`, `--quiet` — Suppress human-readable output; print only canister IDs, one per line, to stdout
* `--cycles <CYCLES>` — Cycles to fund canister creation (in raw cycles)

  Default value: `2000000000000`
* `--subnet <SUBNET>` — The subnet to create canisters on



## `icp-cli canister delete`

**Usage:** `icp-cli canister delete [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name of canister to target

###### **Options:**

* `--network <NETWORK>`
* `--environment <ENVIRONMENT>`
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp-cli canister info`

**Usage:** `icp-cli canister info [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister install`

**Usage:** `icp-cli canister install [OPTIONS] [NAMES]...`

###### **Arguments:**

* `<NAMES>` — The names of the canisters within the current project

###### **Options:**

* `-m`, `--mode <MODE>` — Specifies the mode of canister installation

  Default value: `auto`

  Possible values: `auto`, `install`, `reinstall`, `upgrade`

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister list`

**Usage:** `icp-cli canister list [OPTIONS]`

###### **Options:**

* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister settings`

**Usage:** `icp-cli canister settings <COMMAND>`

###### **Subcommands:**

* `show` — 
* `update` — 



## `icp-cli canister settings show`

**Usage:** `icp-cli canister settings show [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister settings update`

**Usage:** `icp-cli canister settings update [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic
* `--add-controller <ADD_CONTROLLER>`
* `--remove-controller <REMOVE_CONTROLLER>`
* `--set-controller <SET_CONTROLLER>`
* `--compute-allocation <COMPUTE_ALLOCATION>`
* `--memory-allocation <MEMORY_ALLOCATION>`
* `--freezing-threshold <FREEZING_THRESHOLD>`
* `--reserved-cycles-limit <RESERVED_CYCLES_LIMIT>`
* `--wasm-memory-limit <WASM_MEMORY_LIMIT>`
* `--wasm-memory-threshold <WASM_MEMORY_THRESHOLD>`
* `--log-visibility <LOG_VISIBILITY>`
* `--add-log-viewer <ADD_LOG_VIEWER>`
* `--remove-log-viewer <REMOVE_LOG_VIEWER>`
* `--set-log-viewer <SET_LOG_VIEWER>`
* `--add-environment-variable <ADD_ENVIRONMENT_VARIABLE>`
* `--remove-environment-variable <REMOVE_ENVIRONMENT_VARIABLE>`



## `icp-cli canister show`

**Usage:** `icp-cli canister show [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister snapshot`

**Usage:** `icp-cli canister snapshot <COMMAND>`

###### **Subcommands:**

* `create` — 
* `delete` — 
* `download` — 
* `list` — 
* `load` — 
* `upload` — 



## `icp-cli canister snapshot create`

**Usage:** `icp-cli canister snapshot create [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic
* `--replace <REPLACE>` — If a snapshot ID is specified, this snapshot will replace it and reuse the ID



## `icp-cli canister snapshot delete`

**Usage:** `icp-cli canister snapshot delete [OPTIONS] <NAME> <SNAPSHOT>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project
* `<SNAPSHOT>` — The ID of the snapshot to delete

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister snapshot download`

**Usage:** `icp-cli canister snapshot download [OPTIONS] --dir <DIR> <NAME> <SNAPSHOT>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project
* `<SNAPSHOT>` — The ID of the snapshot to download

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic
* `--dir <DIR>` — The directory to download the snapshot to
* `-r`, `--resume` — Whether to resume the download if the previous snapshot download failed

  Default value: `false`
* `--concurrency <CONCURRENCY>` — The number of concurrent downloads to perform

  Default value: `3`



## `icp-cli canister snapshot list`

**Usage:** `icp-cli canister snapshot list [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister snapshot load`

**Usage:** `icp-cli canister snapshot load [OPTIONS] <NAME> <SNAPSHOT>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project
* `<SNAPSHOT>` — The ID of the snapshot to load

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister snapshot upload`

**Usage:** `icp-cli canister snapshot upload [OPTIONS] --dir <DIR> <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic
* `--replace <REPLACE>` — If a snapshot ID is specified, this snapshot will replace it and reuse the ID
* `--dir <DIR>` — The directory to upload the snapshot from
* `-r`, `--resume <RESUME>` — The snapshot ID to resume uploading to
* `--concurrency <CONCURRENCY>` — The number of concurrent uploads to perform

  Default value: `3`



## `icp-cli canister start`

**Usage:** `icp-cli canister start [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister status`

**Usage:** `icp-cli canister status [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name of canister to target

###### **Options:**

* `--network <NETWORK>`
* `--environment <ENVIRONMENT>`
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp-cli canister stop`

**Usage:** `icp-cli canister stop [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister top-up`

**Usage:** `icp-cli canister top-up [OPTIONS] --amount <AMOUNT> <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--amount <AMOUNT>` — Amount of cycles to top up
* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli cycles`

Mint and manage cycles

**Usage:** `icp-cli cycles <COMMAND>`

###### **Subcommands:**

* `balance` — 
* `mint` — 



## `icp-cli cycles balance`

**Usage:** `icp-cli cycles balance [OPTIONS]`

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli cycles mint`

**Usage:** `icp-cli cycles mint [OPTIONS]`

###### **Options:**

* `--icp <ICP>` — Amount of ICP to mint to cycles
* `--cycles <CYCLES>` — Amount of cycles to mint. Automatically determines the amount of ICP needed
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp-cli deploy`

Deploy a project to an environment

**Usage:** `icp-cli deploy [OPTIONS] [NAMES]...`

###### **Arguments:**

* `<NAMES>` — Canister names

###### **Options:**

* `-m`, `--mode <MODE>` — Specifies the mode of canister installation

  Default value: `auto`

  Possible values: `auto`, `install`, `reinstall`, `upgrade`

* `--subnet-id <SUBNET_ID>` — The subnet id to use for the canisters being deployed
* `--controller <CONTROLLER>` — One or more controllers for the canisters being deployed. Repeat `--controller` to specify multiple
* `--cycles <CYCLES>` — Cycles to fund canister creation (in cycles)

  Default value: `2000000000000`
* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli environment`

Show information about the current project environments

**Usage:** `icp-cli environment <COMMAND>`

###### **Subcommands:**

* `list` — 



## `icp-cli environment list`

**Usage:** `icp-cli environment list`



## `icp-cli identity`

Manage your identities

**Usage:** `icp-cli identity <COMMAND>`

###### **Subcommands:**

* `default` — 
* `import` — 
* `list` — 
* `new` — 
* `principal` — 



## `icp-cli identity default`

**Usage:** `icp-cli identity default [NAME]`

###### **Arguments:**

* `<NAME>`



## `icp-cli identity import`

**Usage:** `icp-cli identity import [OPTIONS] <--from-pem <FILE>|--read-seed-phrase|--from-seed-file <FILE>> <NAME>`

###### **Arguments:**

* `<NAME>`

###### **Options:**

* `--from-pem <FILE>`
* `--read-seed-phrase`
* `--from-seed-file <FILE>`
* `--decryption-password-from-file <FILE>`
* `--assert-key-type <ASSERT_KEY_TYPE>`



## `icp-cli identity list`

**Usage:** `icp-cli identity list`



## `icp-cli identity new`

**Usage:** `icp-cli identity new [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>`

###### **Options:**

* `--output-seed <FILE>`



## `icp-cli identity principal`

**Usage:** `icp-cli identity principal [OPTIONS]`

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as



## `icp-cli network`

Launch and manage local test networks

**Usage:** `icp-cli network <COMMAND>`

###### **Subcommands:**

* `list` — List networks in the project
* `ping` — Try to connect to a network, and print out its status
* `run` — Run a given network
* `stop` — Stop a background network



## `icp-cli network list`

List networks in the project

**Usage:** `icp-cli network list`



## `icp-cli network ping`

Try to connect to a network, and print out its status

**Usage:** `icp-cli network ping [OPTIONS] [NETWORK]`

###### **Arguments:**

* `<NETWORK>` — The compute network to connect to. By default, ping the local network

  Default value: `local`

###### **Options:**

* `--wait-healthy` — Repeatedly ping until the replica is healthy or 1 minute has passed



## `icp-cli network run`

Run a given network

**Usage:** `icp-cli network run [OPTIONS] [NAME]`

###### **Arguments:**

* `<NAME>` — Name of the network to run

  Default value: `local`

###### **Options:**

* `--background` — Starts the network in a background process. This command will exit once the network is running. To stop the network, use 'icp network stop'



## `icp-cli network stop`

Stop a background network

**Usage:** `icp-cli network stop [NAME]`

###### **Arguments:**

* `<NAME>` — Name of the network to stop

  Default value: `local`



## `icp-cli sync`

Synchronize canisters in the current environment

**Usage:** `icp-cli sync [OPTIONS] [NAMES]...`

###### **Arguments:**

* `<NAMES>` — Canister names

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli token`

Perform token transactions

**Usage:** `icp-cli token [TOKEN] <COMMAND>`

###### **Subcommands:**

* `balance` — 
* `transfer` — 

###### **Arguments:**

* `<TOKEN>`

  Default value: `icp`



## `icp-cli token balance`

**Usage:** `icp-cli token balance [OPTIONS]`

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli token transfer`

**Usage:** `icp-cli token transfer [OPTIONS] <AMOUNT> <RECEIVER>`

###### **Arguments:**

* `<AMOUNT>` — Token amount to transfer
* `<RECEIVER>` — The receiver of the token transfer

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>

