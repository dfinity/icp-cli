# icp cycles mint

Convert ICP to cycles.

Exactly one of --icp or --cycles must be provided.

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
* `--json` — Output command results as JSON




