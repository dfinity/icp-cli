# icp canister call

Make a canister call

**Usage:** `icp canister call [OPTIONS] <CANISTER> <METHOD> [ARGS]`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified
* `<METHOD>` — Name of canister method to call into
* `<ARGS>` — Canister call arguments. Can be:

   - Hex-encoded bytes (e.g., `4449444c00`)

   - Candid text format (e.g., `(42)` or `(record { name = "Alice" })`)

   - File path (e.g., `args.txt` or `./path/to/args.candid`) The file should contain either hex or Candid format arguments.

   If not provided, an interactive prompt will be launched to help build the arguments.

###### **Options:**

* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--proxy <PROXY>` — Principal of a proxy canister to route the call through.

   When specified, instead of calling the target canister directly, the call will be sent to the proxy canister's `proxy` method, which forwards it to the target canister.
* `--cycles <CYCLES>` — Cycles to forward with the proxied call.

   Only used when --proxy is specified. Defaults to 0.

  Default value: `0`




