# icp canister status

Show the status of canister(s).

By default this queries the status endpoint of the management canister. If the caller is not a controller, falls back on fetching public information from the state tree.

**Usage:** `icp canister status [OPTIONS] [CANISTER]`

Examples:

    # Status of all canisters in the local environment
    icp canister status

    # Status of one canister by name
    icp canister status backend -e local

    # Print only canister IDs (useful for scripting)
    icp canister status -i

    # JSON output for all canisters
    icp canister status --json


###### **Arguments:**

* `<CANISTER>` — An optional canister name or principal to target. When using a name, an environment must be specified. If omitted, shows status for all canisters in the environment

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `-i`, `--id-only` — Only print the canister ids
* `--json` — Format output in json
* `-p`, `--public` — Show the only the public information. Skips trying to get the status from the management canister and looks up public information from the state tree
* `--proxy <PROXY>` — Principal of a proxy canister to route the management canister call through




