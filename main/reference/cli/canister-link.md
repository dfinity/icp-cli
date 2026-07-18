# icp canister link

Link an existing canister to the project by recording its ID in the canister ID store.

This associates an already-deployed canister with a name declared in the target environment, without creating a new canister. It is the inverse of the record that `icp canister create` writes automatically.

**Usage:** `icp canister link [OPTIONS] <NAME> <PRINCIPAL>`

###### **Arguments:**

* `<NAME>` — Name of the project canister to associate the ID with. Must be declared in the target environment
* `<PRINCIPAL>` — Principal of the existing canister to link

###### **Options:**

* `-e`, `--environment <ENVIRONMENT>` — Override the environment to connect to. By default, the local environment is used
* `--force` — Overwrite an ID already recorded for this canister




