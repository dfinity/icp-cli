# icp build

Build canisters

**Usage:** `icp build [OPTIONS] [CANISTERS]...`

###### **Arguments:**

* `<CANISTERS>` — Canister names (if empty, build all canisters in environment)

###### **Options:**

* `-e`, `--environment <ENVIRONMENT>` — Override the environment to build for. By default, the local environment is used
* `--untrusted` — Hold the project to what one someone else handed over may do: every file it reads is under the project root and reached through no symlink, each build step is a pre-built module and each sync step a plugin, and it uses no recipe. Anything else is refused. Run it from the directory that holds `icp.yaml`: that directory is the project root, and no root is looked for in the directories above it. Name another directory with `--project-root-override`. The `.icp` directory is the tool's own state and is neither confined nor checked: delete one that came with the project before running




