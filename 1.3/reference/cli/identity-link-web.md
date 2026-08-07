# icp identity link web

Link a web-based identity (such as Internet Identity) to a new icp-cli identity

**Usage:** `icp identity link web [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — Name for the linked identity

###### **Options:**

* `--auth <AUTH>` — Auth domain to sign in at (e.g. id.ai or identity.ce1.com). Its `/.well-known/cli-auth-config` decides the login path

  Default value: `https://id.ai`
* `--app <APP>` — Delegation domain to get an identity for (e.g. oisy.com). When omitted, the auth domain picks its default (id.ai uses cli.id.ai)
* `--storage <STORAGE>` — Where to store the session private key

  Default value: `keyring`

  Possible values: `plaintext`, `keyring`, `password`

* `--storage-password-file <FILE>` — Read the storage password from a file instead of prompting (for --storage password)




