# icp canister call

Make a canister call

**Usage:** `icp canister call [OPTIONS] <CANISTER> [METHOD] [ARGS]`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified
* `<METHOD>` — Name of canister method to call into. If not provided, an interactive prompt will be launched
* `<ARGS>` — Call arguments, interpreted per `--args-format` (Candid by default). If not provided, an interactive prompt will be launched

###### **Options:**

* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`. One of `mainnet`, `fetch`, or a 266-character hex-encoded root key
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

* `--candid <PATH>` — Path to a Candid (`.did`) file describing the canister's interface.

   When set, this interface is used to assist method selection, build arguments, and decode the response, instead of fetching the canister's Candid interface from the network.
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
* `--sign-only <FILE>` — Sign the call and write it to FILE instead of submitting it, so it can be submitted later from a machine that has network access but not your key. `-` writes to stdout.

   Nothing is sent, and nothing is fetched: the interface comes from `--candid` or the local build artifact rather than from the canister, so this works with no network at all. `--root-key` must name a key rather than `fetch`, and `--proxy` is not supported.
* `--valid-from <WHEN>` — When the signed message's five-minute submission window opens: a duration from now (`55m`, `2h`) or an RFC 3339 timestamp (`2026-08-17T10:07:00Z`). Defaults to now.

   The window is always five minutes wide — the IC will not accept an ingress message expiring further ahead than that — so this places it rather than sizing it. It is rounded down to the whole minute, and so may open up to 59 seconds earlier than asked; the file records the window it actually got.




