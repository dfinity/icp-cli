# icp canister install

Install a built WASM to a canister on a network

**Usage:** `icp canister install [OPTIONS] <CANISTER>`

###### **Arguments:**

* `<CANISTER>` — Name or principal of canister to target When using a name an environment must be specified

###### **Options:**

* `-m`, `--mode <MODE>` — Specifies the mode of canister installation

  Default value: `auto`

  Possible values: `auto`, `install`, `reinstall`, `upgrade`

* `--wasm <WASM>` — Path to the WASM file to install. Uses the build output if not explicitly provided
* `--args <ARGS>` — Initialization arguments for the canister. Can be:

   - Hex-encoded bytes (e.g., `4449444c00`)

   - Candid text format (e.g., `(42)` or `(record { name = "Alice" })`)

   - File path (e.g., `args.txt` or `./path/to/args.candid`) The file should contain either hex or Candid format arguments.
* `-n`, `--network <NETWORK>` — Name of the network to target, conflicts with environment argument
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as




