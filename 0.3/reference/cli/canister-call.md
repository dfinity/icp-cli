# icp canister call

Make a canister call

**Usage:** `icp canister call [OPTIONS] <CANISTER> [METHOD] [ARGS]`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified
* `<METHOD>` — Name of canister method to call into. If not provided, an interactive prompt will be launched
* `<ARGS>` — Call arguments, interpreted per `--args-format` (Candid by default). If not provided, an interactive prompt will be launched

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--args-file <ARGS_FILE>` — Path to a file containing call arguments
* `--args-format <ARGS_FORMAT>` — Format of the call arguments

  Default value: `candid`

  Possible values:
  - `hex`:
    Hex-encoded bytes
  - `candid`:
    Candid text format
  - `bin`:
    Raw binary (only valid for file references)

* `--proxy <PROXY>` — Principal of a proxy canister to route the call through.

   When specified, instead of calling the target canister directly, the call will be sent to the proxy canister's `proxy` method, which forwards it to the target canister.
* `--cycles <CYCLES>` — Cycles to forward with the proxied call.

   Only used when --proxy is specified. Defaults to 0.

  Default value: `0`
* `--query` — Sends a query request to a canister instead of an update request.

   Query calls are faster but return uncertified responses. Cannot be used with --proxy (proxy calls are always update calls).
* `-o`, `--output <OUTPUT>` — How to interpret and display the response

  Default value: `auto`

  Possible values:
  - `auto`:
    Try Candid, then UTF-8, then fall back to hex
  - `candid`:
    Parse as Candid and pretty-print; error if parsing fails
  - `text`:
    Parse as UTF-8 text; error if invalid
  - `hex`:
    Print raw response as hex

* `--json` — Output command results as JSON




