# icp extension install

Install an extension as a command of icp, or replace an installed one

**Usage:** `icp extension install [OPTIONS] <NAME> <SOURCE>`

###### **Arguments:**

* `<NAME>` — The command to install it as: `icp <NAME>`. Usually the name the extension gives itself, but any name works, e.g. `cyclepay-beta` to try a build beside the installed one
* `<SOURCE>` — The extension's wasm component: an https URL or a local path

###### **Options:**

* `--sha256 <SHA256>` — Refuse the install unless the component has this sha256 (hex)
* `-y`, `--yes` — Approve the install without asking




