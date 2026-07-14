# icp token approve

Approve a spender to transfer tokens on your behalf (ICRC-2) (default token: icp)

Sets the spender's allowance to the given amount, overwriting any existing allowance (this is a set, not an increment). The allowance is granted from the calling identity's account, which is charged the ledger's approval fee, and can optionally be given an expiry with `--expires-in`. Works with any ICRC-2 ledger, referenced by a known token name or a ledger canister id.

**Usage:** `icp token [TOKEN|LEDGER_ID] approve [OPTIONS] <AMOUNT> <SPENDER>`

###### **Arguments:**

* `<AMOUNT>` — The allowance amount, in whole tokens (e.g. `1.5`), the spender may transfer. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)
* `<SPENDER>` — Principal of the spender being granted the allowance

###### **Options:**

* `--spender-subaccount <SPENDER_SUBACCOUNT>` — The spender's subaccount, as a hex string (32 bytes, left-padded). Defaults to the default subaccount
* `--from-subaccount <FROM_SUBACCOUNT>` — The caller's subaccount to grant the allowance from (the account debited), as a hex string (32 bytes, left-padded). Defaults to the default subaccount
* `--expires-in <DURATION>` — Expire the allowance after this duration from now, e.g. `24h`, `30d` (suffixes: s, m, h, d, w; a bare number is seconds). Never expires if omitted
* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`. One of `mainnet`, `fetch`, or a 266-character hex-encoded root key
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--json` — Output command results as JSON
* `-q`, `--quiet` — Suppress human-readable output; print only the block index




