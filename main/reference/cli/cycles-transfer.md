# icp cycles transfer

Transfer cycles to another principal

**Usage:** `icp cycles transfer [OPTIONS] <AMOUNT> <RECEIVER>`

###### **Arguments:**

* `<AMOUNT>` — Cycles amount to transfer. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `<RECEIVER>` — The receiver of the cycles transfer

###### **Options:**

* `--to-subaccount <TO_SUBACCOUNT>` — The subaccount to transfer to (only if the receiver is a principal)
* `--from-subaccount <FROM_SUBACCOUNT>` — The subaccount to transfer cycles from
* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--json` — Output command results as JSON
* `-q`, `--quiet` — Suppress human-readable output; print only the block index




