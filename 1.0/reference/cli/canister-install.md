# icp canister install

Install a built WASM to a canister on a network

**Usage:** `icp canister install [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target. When using a name an environment must be specified

###### **Options:**

* `-m`, `--mode <MODE>` — Specifies the mode of canister installation

  Default value: `auto`

  Possible values: `auto`, `install`, `reinstall`, `upgrade`

* `--wasm-memory-persistence <WASM_MEMORY_PERSISTENCE>` — For Motoko canisters with enhanced orthogonal persistence (EOP), controls whether the canister's main (Wasm) memory is preserved across an upgrade.

   Only valid with `--mode upgrade` on an EOP canister.

   - `keep`: preserve main memory — the normal EOP upgrade (the default if this flag is omitted).

   - `replace`: discard main memory. DANGEROUS: any state not held in `stable` variables is lost. Requires interactive confirmation (or `--yes`).

  Possible values:
  - `keep`:
    Preserve canister main memory across upgrade (normal EOP upgrade)
  - `replace`:
    Discard canister main memory; only `stable` variables survive. Dangerous — heap state is lost

* `--wasm <WASM>` — Path to the WASM file to install. Uses the build output if not explicitly provided
* `--args <ARGS>` — Inline arguments, interpreted per `--args-format` (Candid by default)
* `--args-file <ARGS_FILE>` — Path to a file containing arguments
* `--args-format <ARGS_FORMAT>` — Format of the arguments

  Default value: `candid`

  Possible values:
  - `hex`:
    Hex-encoded bytes
  - `candid`:
    Candid text format
  - `bin`:
    Raw binary (only valid for file references)

* `-y`, `--yes` — Skip confirmation prompts, including the Candid interface compatibility check and the dangerous-operation prompt for `--wasm-memory-persistence replace`
* `-n`, `--network <NETWORK>` — Name or URL of the network to target, conflicts with environment argument
* `-k`, `--root-key <ROOT_KEY>` — The root key to use if connecting to a network by URL. Required when using `--network <URL>`
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--proxy <PROXY>` — Principal of a proxy canister to route the management canister call through




