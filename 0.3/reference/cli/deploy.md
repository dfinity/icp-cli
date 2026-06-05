# icp deploy

Deploy a project to an environment

**Usage:** `icp deploy [OPTIONS] [NAMES]...`

When deploying a single canister, you can pass arguments to the install call
using --args or --args-file:

    # Pass inline Candid arguments
    icp deploy my_canister --args '(42 : nat)'

    # Pass arguments from a file
    icp deploy my_canister --args-file ./args.did

    # Pass raw bytes
    icp deploy my_canister --args-file ./args.bin --args-format bin


###### **Arguments:**

* `<NAMES>` — Canister names

###### **Options:**

* `-m`, `--mode <MODE>` — Specifies the mode of canister installation

  Default value: `auto`

  Possible values: `auto`, `install`, `reinstall`, `upgrade`

* `--subnet <SUBNET>` — The subnet to use for the canisters being deployed
* `--proxy <PROXY>` — Principal of a proxy canister to route management canister calls through
* `--controller <CONTROLLER>` — One or more controllers for the canisters being deployed. Repeat `--controller` to specify multiple
* `--cycles <CYCLES>` — Cycles to fund canister creation. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)

  Default value: `2000000000000`
* `-y`, `--yes` — Skip confirmation prompts, including the Candid interface compatibility check
* `--identity <IDENTITY>` — The user identity to run this command as
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--json` — Output command results as JSON
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





