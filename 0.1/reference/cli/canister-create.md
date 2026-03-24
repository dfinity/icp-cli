# icp canister create

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
* `--cycles <CYCLES>` — Cycles to fund canister creation. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)

  Default value: `2000000000000`
* `--subnet <SUBNET>` — The subnet to create canisters on




