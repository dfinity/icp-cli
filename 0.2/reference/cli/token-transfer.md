# icp token transfer

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

