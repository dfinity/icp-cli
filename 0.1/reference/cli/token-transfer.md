# icp token transfer

Transfer ICP or ICRC1 tokens through their ledger (default token: icp)

**Usage:** `icp token [TOKEN|LEDGER_ID] transfer [OPTIONS] <AMOUNT> <RECEIVER>`

###### **Arguments:**

* `<AMOUNT>` — Token amount to transfer. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `<RECEIVER>` — The receiver of the token transfer. Can be a Principal or an AccountIdentifier hex string (only for ICP ledger)

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as

