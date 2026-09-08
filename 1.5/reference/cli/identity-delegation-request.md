# icp identity delegation request

Create a pending delegation identity with a new P256 session key

Prints the session public key as a PEM-encoded SPKI to stdout. Pass this to `icp identity delegation sign --key-pem` on another machine to obtain a delegation chain, then complete the identity with `icp identity delegation use`.

**Usage:** `icp identity delegation request [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — Name for the new identity

###### **Options:**

* `--storage <STORAGE>` — Where to store the session private key

  Default value: `keyring`

  Possible values: `plaintext`, `keyring`, `password`

* `--storage-password-file <FILE>` — Read the storage password from a file instead of prompting (for --storage password)




