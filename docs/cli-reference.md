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
* [`icp-cli canister show`↴](#icp-cli-canister-show)
* [`icp-cli canister list`↴](#icp-cli-canister-list)
* [`icp-cli canister start`↴](#icp-cli-canister-start)
* [`icp-cli canister status`↴](#icp-cli-canister-status)
* [`icp-cli canister stop`↴](#icp-cli-canister-stop)
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
* [`icp-cli sync`↴](#icp-cli-sync)
* [`icp-cli token`↴](#icp-cli-token)
* [`icp-cli token balance`↴](#icp-cli-token-balance)
* [`icp-cli token transfer`↴](#icp-cli-token-transfer)

## `icp-cli`

**Usage:** `icp-cli [OPTIONS] [COMMAND]`

###### **Subcommands:**

* `build` — 
* `canister` — 
* `cycles` — 
* `deploy` — 
* `environment` — 
* `identity` — 
* `network` — 
* `sync` — 
* `token` — 

###### **Options:**

* `--id-store <ID_STORE>`

  Default value: `.icp/ids.json`
* `--artifact-store <ARTIFACT_STORE>`

  Default value: `.icp/artifacts`
* `--debug` — Enable debug logging

  Default value: `false`



## `icp-cli build`

**Usage:** `icp-cli build [NAMES]...`

###### **Arguments:**

* `<NAMES>` — The names of the canisters within the current project



## `icp-cli canister`

**Usage:** `icp-cli canister <COMMAND>`

###### **Subcommands:**

* `call` — 
* `create` — 
* `delete` — 
* `info` — 
* `install` — 
* `show` — 
* `list` — 
* `start` — 
* `status` — 
* `stop` — 



## `icp-cli canister call`

**Usage:** `icp-cli canister call [OPTIONS] <NAME> <METHOD> <ARGS>`

###### **Arguments:**

* `<NAME>` — Name of canister to call to
* `<METHOD>` — Name of canister method to call into
* `<ARGS>` — String representation of canister call arguments

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister create`

**Usage:** `icp-cli canister create [OPTIONS] [NAMES]...`

###### **Arguments:**

* `<NAMES>` — The names of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic
* `--effective-id <EFFECTIVE_ID>` — The effective canister ID to use when calling the management canister

  Default value: `tqzl2-p7777-77776-aaaaa-cai`
* `--specific-id <SPECIFIC_ID>` — The specific canister ID to assign if creating with a fixed principal
* `--controller <CONTROLLER>` — One or more controllers for the canister. Repeat `--controller` to specify multiple
* `--compute-allocation <COMPUTE_ALLOCATION>` — Optional compute allocation (0 to 100). Represents guaranteed compute capacity
* `--memory-allocation <MEMORY_ALLOCATION>` — Optional memory allocation in bytes. If unset, memory is allocated dynamically
* `--freezing-threshold <FREEZING_THRESHOLD>` — Optional freezing threshold in seconds. Controls how long a canister can be inactive before being frozen
* `--reserved-cycles-limit <RESERVED_CYCLES_LIMIT>` — Optional reserved cycles limit. If set, the canister cannot consume more than this many cycles
* `--wasm-memory-limit <WASM_MEMORY_LIMIT>` — Optional Wasm memory limit in bytes. Sets an upper bound for Wasm heap growth
* `--wasm-memory-threshold <WASM_MEMORY_THRESHOLD>` — Optional Wasm memory threshold in bytes. Triggers a callback when exceeded
* `-q`, `--quiet` — Suppress human-readable output; print only canister IDs, one per line, to stdout



## `icp-cli canister delete`

**Usage:** `icp-cli canister delete [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



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



## `icp-cli canister show`

**Usage:** `icp-cli canister show [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister list`

**Usage:** `icp-cli canister list [OPTIONS]`

###### **Options:**

* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister start`

**Usage:** `icp-cli canister start [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister status`

**Usage:** `icp-cli canister status [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli canister stop`

**Usage:** `icp-cli canister stop [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli cycles`

**Usage:** `icp-cli cycles <COMMAND>`

###### **Subcommands:**

* `balance` — 
* `mint` — 



## `icp-cli cycles balance`

**Usage:** `icp-cli cycles balance`



## `icp-cli cycles mint`

**Usage:** `icp-cli cycles mint`



## `icp-cli deploy`

**Usage:** `icp-cli deploy [OPTIONS] [NAME]`

###### **Arguments:**

* `<NAME>` — The name of the canister within the current project

###### **Options:**

* `-m`, `--mode <MODE>` — Specifies the mode of canister installation

  Default value: `auto`

  Possible values: `auto`, `install`, `reinstall`, `upgrade`

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic
* `--subnet-id <SUBNET_ID>` — The subnet id to use for the canisters being deployed
* `--controller <CONTROLLER>` — One or more controllers for the canisters being deployed. Repeat `--controller` to specify multiple



## `icp-cli environment`

**Usage:** `icp-cli environment <COMMAND>`

###### **Subcommands:**

* `list` — 



## `icp-cli environment list`

**Usage:** `icp-cli environment list`



## `icp-cli identity`

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

  Possible values: `secp256k1`




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

**Usage:** `icp-cli network <COMMAND>`

###### **Subcommands:**

* `list` — List networks in the project
* `ping` — Try to connect to a network, and print out its status
* `run` — Run a given network



## `icp-cli network list`

List networks in the project

**Usage:** `icp-cli network list`



## `icp-cli network ping`

Try to connect to a network, and print out its status

**Usage:** `icp-cli network ping [OPTIONS] [NETWORK]`

###### **Arguments:**

* `<NETWORK>` — The compute network to connect to. By default, ping the local network

###### **Options:**

* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic
* `--wait-healthy` — Repeatedly ping until the replica is healthy or 1 minute has passed



## `icp-cli network run`

Run a given network

**Usage:** `icp-cli network run [NAME]`

###### **Arguments:**

* `<NAME>` — Name of the network to run

  Default value: `local`



## `icp-cli sync`

**Usage:** `icp-cli sync [OPTIONS] [NAMES]...`

###### **Arguments:**

* `<NAMES>` — The names of the canisters within the current project

###### **Options:**

* `--identity <IDENTITY>` — The user identity to run this command as
* `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--ic` — Shorthand for --environment=ic



## `icp-cli token`

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

