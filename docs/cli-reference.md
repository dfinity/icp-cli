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
* [`icp canister metadata`↴](#icp-canister-metadata)
* [`icp canister settings`↴](#icp-canister-settings)
* [`icp canister settings show`↴](#icp-canister-settings-show)
* [`icp canister settings update`↴](#icp-canister-settings-update)
* [`icp canister settings sync`↴](#icp-canister-settings-sync)
* [`icp canister start`↴](#icp-canister-start)
* [`icp canister status`↴](#icp-canister-status)
* [`icp canister stop`↴](#icp-canister-stop)
* [`icp canister top-up`↴](#icp-canister-top-up)
* [`icp cycles`↴](#icp-cycles)
* [`icp cycles balance`↴](#icp-cycles-balance)
* [`icp cycles mint`↴](#icp-cycles-mint)
* [`icp deploy`↴](#icp-deploy)
* [`icp environment`↴](#icp-environment)
* [`icp environment list`↴](#icp-environment-list)
* [`icp identity`↴](#icp-identity)
* [`icp identity default`↴](#icp-identity-default)
* [`icp identity import`↴](#icp-identity-import)
* [`icp identity list`↴](#icp-identity-list)
* [`icp identity new`↴](#icp-identity-new)
* [`icp identity principal`↴](#icp-identity-principal)
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
* `metadata` — Read a metadata section from a canister
* `settings` — Commands to manage canister settings
* `start` — Start a canister on a network
* `status` — Show the status of canister(s)
* `stop` — Stop a canister on a network
* `top-up` — Top up a canister with cycles



## `icp canister call`

Make a canister call

**Usage:** `icp canister call [OPTIONS] <CANISTER> <METHOD> [ARGS]`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified
* `<METHOD>` — Name of canister method to call into
* `<ARGS>` — String representation of canister call arguments

   If not provided, an interactive prompt will be launched to help build the arguments.

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister create`

Create a canister on a network

**Usage:** `icp canister create [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--controller <CONTROLLER>` — One or more controllers for the canister. Repeat `--controller` to specify multiple
* `--compute-allocation <COMPUTE_ALLOCATION>` — Optional compute allocation (0 to 100). Represents guaranteed compute capacity
* `--memory-allocation <MEMORY_ALLOCATION>` — Optional memory allocation in bytes. If unset, memory is allocated dynamically
* `--freezing-threshold <FREEZING_THRESHOLD>` — Optional freezing threshold in seconds. Controls how long a canister can be inactive before being frozen
* `--reserved-cycles-limit <RESERVED_CYCLES_LIMIT>` — Optional reserved cycles limit. If set, the canister cannot consume more than this many cycles
* `-q`, `--quiet` — Suppress human-readable output; print only canister IDs, one per line, to stdout
* `--cycles <CYCLES>` — Cycles to fund canister creation (in raw cycles)

  Default value: `2000000000000`
* `--subnet <SUBNET>` — The subnet to create canisters on



## `icp canister delete`

Delete a canister from a network

**Usage:** `icp canister delete [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister install`

Install a built WASM to a canister on a network

**Usage:** `icp canister install [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified

###### **Options:**

* `-m`, `--mode <MODE>` — Specifies the mode of canister installation

  Default value: `auto`

  Possible values: `auto`, `install`, `reinstall`, `upgrade`

* `--wasm <WASM>` — Path to the WASM file to install. Uses the build output if not explicitly provided
* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister list`

List the canisters in an environment

**Usage:** `icp canister list [OPTIONS]`

###### **Options:**

* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used



## `icp canister metadata`

Read a metadata section from a canister

**Usage:** `icp canister metadata [OPTIONS] <CANISTER> <METADATA_NAME>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified
* `<METADATA_NAME>` — The name of the metadata section to read

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



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

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `-i`, `--id-only` — Only print the canister ids
* `--json` — Format output in json
* `-p`, `--public` — Show the only the public information. Skips trying to get the status from the management canister and looks up public information from the state tree



## `icp canister settings update`

Change a canister's settings to specified values

**Usage:** `icp canister settings update [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
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



## `icp canister settings sync`

Synchronize a canister's settings with those defined in the project

**Usage:** `icp canister settings sync [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister start`

Start a canister on a network

**Usage:** `icp canister start [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister status`

Show the status of canister(s).

By default this queries the status endpoint of the management canister. If the caller is not a controller, falls back on fetching public information from the state tree.

**Usage:** `icp canister status [OPTIONS] [CANISTER]`

###### **Arguments:**

* `<CANISTER>` — An optional canister name or principal to target. When using a name, an enviroment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `-i`, `--id-only` — Only print the canister ids
* `--json` — Format output in json
* `-p`, `--public` — Show the only the public information. Skips trying to get the status from the management canister and looks up public information from the state tree



## `icp canister stop`

Stop a canister on a network

**Usage:** `icp canister stop [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp canister top-up`

Top up a canister with cycles

**Usage:** `icp canister top-up [OPTIONS] --amount <AMOUNT> <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified

###### **Options:**

* `--amount <AMOUNT>` — Amount of cycles to top up
* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp cycles`

Mint and manage cycles

**Usage:** `icp cycles <COMMAND>`

###### **Subcommands:**

* `balance` — Display the cycles balance
* `mint` — Convert icp to cycles



## `icp cycles balance`

Display the cycles balance

**Usage:** `icp cycles balance [OPTIONS]`

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp cycles mint`

Convert icp to cycles

**Usage:** `icp cycles mint [OPTIONS]`

###### **Options:**

* `--icp <ICP>` — Amount of ICP to mint to cycles
* `--cycles <CYCLES>` — Amount of cycles to mint. Automatically determines the amount of ICP needed
* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
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
* `--cycles <CYCLES>` — Cycles to fund canister creation (in cycles)

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

* `default` — Display the currently selected identity
* `import` — Import a new identity
* `list` — List the identities
* `new` — Create a new identity
* `principal` — Display the principal for the current identity



## `icp identity default`

Display the currently selected identity

**Usage:** `icp identity default [NAME]`

###### **Arguments:**

* `<NAME>` — Identity to set as default. If omitted, prints the current default



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



## `icp network`

Launch and manage local test networks

**Usage:** `icp network <COMMAND>`

###### **Subcommands:**

* `list` — 
* `ping` — Try to connect to a network, and print out its status
* `start` — Run a given network
* `status` — Get status information about a running network
* `stop` — Stop a background network
* `update` — Update icp-cli-network-launcher to the latest version



## `icp network list`

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

**Usage:** `icp token [TOKEN] <COMMAND>`

###### **Subcommands:**

* `balance` — 
* `transfer` — 

###### **Arguments:**

* `<TOKEN>` — The token to execute the operation on, defaults to `icp`

  Default value: `icp`



## `icp token balance`

**Usage:** `icp token balance [OPTIONS]`

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



## `icp token transfer`

**Usage:** `icp token transfer [OPTIONS] <AMOUNT> <RECEIVER>`

###### **Arguments:**

* `<AMOUNT>` — Token amount to transfer
* `<RECEIVER>` — The receiver of the token transfer

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>

