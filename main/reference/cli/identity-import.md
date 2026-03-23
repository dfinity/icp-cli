# icp identity import

Import a new identity

**Usage:** `icp identity import [OPTIONS] <--from-pem <FILE>|--read-seed-phrase|--from-seed-file <FILE>> <NAME>`

###### **Arguments:**

* `<NAME>` — Name for the imported identity

###### **Options:**

* `--storage <STORAGE>` — Where to store the private key

  Default value: `keyring`

  Possible values: `plaintext`, `keyring`, `password`

* `--from-pem <FILE>` — Import from a PEM file
* `--read-seed-phrase` — Read seed phrase interactively from the terminal
* `--from-seed-file <FILE>` — Read seed phrase from a file
* `--decryption-password-from-file <FILE>` — Read the PEM decryption password from a file instead of prompting
* `--storage-password-file <FILE>` — Read the storage password from a file instead of prompting (for --storage password)
* `--assert-key-type <ASSERT_KEY_TYPE>` — Specify the key type when it cannot be detected from the PEM file (danger!)

  Possible values: `secp256k1`, `prime256v1`, `ed25519`





