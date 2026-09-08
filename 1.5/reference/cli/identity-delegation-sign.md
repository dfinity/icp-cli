# icp identity delegation sign

Sign a delegation from the selected identity to a target key

**Usage:** `icp identity delegation sign [OPTIONS] --key-pem <FILE> --duration <DURATION>`

###### **Options:**

* `--key-pem <FILE>` — Public key PEM file of the key to delegate to
* `--duration <DURATION>` — Delegation validity duration (e.g. "30d", "24h", "3600s", or plain seconds)
* `--canisters <CANISTERS>` — Canister principals to restrict the delegation to (comma-separated)
* `--identity <IDENTITY>` — The user identity to run this command as




