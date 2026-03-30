# icp canister create

Create a canister on a network

**Usage:** `icp canister create [OPTIONS] <CANISTER|--detached>`

This command can be used to create canisters defined in a project
or a "detached" canister on a network.

Examples:

    # Create on a network by url
    icp canister create -n http://localhost:8000 -k $ROOT_KEY --detached

    # Create on mainnet outside of a project context
    icp canister create -n ic --detached

    # Create a detached canister inside the scope of a project
    icp canister create -n mynetwork --detached


###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--controller <CONTROLLER>` — One or more controllers for the canister. Repeat `--controller` to specify multiple
* `--compute-allocation <COMPUTE_ALLOCATION>` — Optional compute allocation (0 to 100). Represents guaranteed compute capacity
* `--memory-allocation <MEMORY_ALLOCATION>` — Optional memory allocation in bytes. If unset, memory is allocated dynamically. Supports suffixes: kb, kib, mb, mib, gb, gib (e.g. "4gib" or "2.5kb")
* `--freezing-threshold <FREEZING_THRESHOLD>` — Optional freezing threshold. Controls how long a canister can be inactive before being frozen. Supports duration suffixes: s (seconds), m (minutes), h (hours), d (days), w (weeks). A bare number is treated as seconds
* `--reserved-cycles-limit <RESERVED_CYCLES_LIMIT>` — Optional upper limit on cycles reserved for future resource payments. Memory allocations that would push the reserved balance above this limit will fail. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `-q`, `--quiet` — Suppress human-readable output; print only canister IDs, one per line, to stdout
* `--cycles <CYCLES>` — Cycles to fund canister creation. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)

  Default value: `2000000000000`
* `--subnet <SUBNET>` — The subnet to create canisters on
* `--detached` — Create a canister detached from any project configuration. The canister id will be printed out but not recorded in the project configuration. Not valid if `Canister` is provided




