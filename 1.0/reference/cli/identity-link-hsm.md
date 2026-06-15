# icp identity link hsm

Link an HSM key to a new identity

**Usage:** `icp identity link hsm [OPTIONS] --pkcs11-module <PKCS11_MODULE> --key-id <KEY_ID> <NAME>`

###### **Arguments:**

* `<NAME>` — Name for the linked identity

###### **Options:**

* `--pkcs11-module <PKCS11_MODULE>` — Path to the PKCS#11 module (shared library) for the HSM
* `--slot <SLOT>` — Slot index on the HSM device

  Default value: `0`
* `--key-id <KEY_ID>` — Key ID on the HSM (e.g., "01" for PIV authentication key)
* `--pin-file <PIN_FILE>` — Read HSM PIN from a file instead of prompting




