# Unreleased

* fix: Use EOP when upgrading motoko canisters
* feat: Add `icp network status` command to display network information
  * Displays port, root key, and candid UI principal (if available)
  * Supports `--json` flag for JSON output
* feat: Allow overriding the `local` network in the config file
  * This makes it more convenient to configure the default environment

# v0.1.0-beta.2

* feat: Add support for launching dockerized local networks (#233)
* fix: When deleting a canister, also delete the id from the id store.
* chore!: rename `icp network run` to `icp network start
* feat: install Candid UI canister after starting a local network

# v0.1.0-beta.1

* feat!: Switch to using icp-cli-network-launcher instead of pocket-ic directly. Download it [here](https://github.com/dfinity/icp-cli-network-launcher/releases).
* feat!: Introduce `new` command to create projects from templates (#219)

# v0.1.0-beta.0

This is a the first beta release of icp-cli.

Supports:
* Creating an identity.
* Launching a local network with pocket-ic.
* Executing operations against a network.
* Building and deploying canisters to a network.

