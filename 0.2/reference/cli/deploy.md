# icp deploy

Deploy a project to an environment

**Usage:** `icp deploy [OPTIONS] [NAMES]...`

###### **Arguments:**

* `<NAMES>` — Canister names

###### **Options:**

* `-m`, `--mode <MODE>` — Specifies the mode of canister installation

  Default value: `auto`

  Possible values: `auto`, `install`, `reinstall`, `upgrade`

* `--subnet <SUBNET>` — The subnet to use for the canisters being deployed
* `--controller <CONTROLLER>` — One or more controllers for the canisters being deployed. Repeat `--controller` to specify multiple
* `--cycles <CYCLES>` — Cycles to fund canister creation. Supports suffixes: k (thousand), m (million), b (billion), t (trillion)

  Default value: `2000000000000`
* `-y`, `--yes` — Skip confirmation prompts, including the Candid interface compatibility check
* `--identity <IDENTITY>` — The user identity to run this command as
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used




