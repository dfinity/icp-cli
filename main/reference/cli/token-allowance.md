# icp token allowance

Display the allowance granted to a spender (ICRC-2) (default token: icp)

This is a read-only query that works for any owner/spender pair, including accounts you do not control (use `--of-principal` to set the owner). The amount is shown in whole tokens, along with an expiry if one was set. Works with any ICRC-2 ledger, referenced by a known token name or a ledger canister id.

**Usage:** `icp token [TOKEN|LEDGER_ID] allowance [OPTIONS] <SPENDER>`

###### **Arguments:**

* `<SPENDER>` — Principal of the spender whose allowance to look up

###### **Options:**

* `--spender-subaccount <SPENDER_SUBACCOUNT>` — The spender's subaccount, as a hex string (32 bytes, left-padded). Defaults to the default subaccount
* `--subaccount <SUBACCOUNT>` — The owner's subaccount that granted the allowance, as a hex string (32 bytes, left-padded). Defaults to the default subaccount
* `--of-principal <OF_PRINCIPAL>` — The allowance owner to look up, instead of the current identity. Lets you inspect allowances granted by any principal
* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--json` — Output command results as JSON
* `-q`, `--quiet` — Suppress human-readable output; print only the allowance amount

