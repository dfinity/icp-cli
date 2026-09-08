# icp identity delegation use

Complete a pending delegation identity by providing a signed delegation chain

Reads the JSON output of `icp identity delegation sign` from a file and attaches it to the named identity, making it usable for signing.

**Usage:** `icp identity delegation use --from-json <FILE> <NAME>`

###### **Arguments:**

* `<NAME>` — Name of the pending delegation identity to complete

###### **Options:**

* `--from-json <FILE>` — Path to the delegation chain JSON file (output of `icp identity delegation sign`)




