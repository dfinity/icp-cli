# icp sync

Synchronize canisters

**Usage:** `icp sync [OPTIONS] [CANISTERS]...`

###### **Arguments:**

* `<CANISTERS>` — Canister names (if empty, sync all canisters in environment)

###### **Options:**

* `--proxy <PROXY>` — Principal of a proxy canister to route sync plugin calls to the target canister through
* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--identity <IDENTITY>` — The user identity to run this command as
* `--untrusted` — Hold the project to what one someone else handed over may do: every file it reads is under the project root and reached through no symlink, each build step is a pre-built module and each sync step a plugin, and it uses no recipe. Anything else is refused. Run it from the directory that holds `icp.yaml`: that directory is the project root, and no root is looked for in the directories above it. Name another directory with `--project-root-override`. The `.icp` directory is the tool's own state and is neither confined nor checked: delete one that came with the project before running




