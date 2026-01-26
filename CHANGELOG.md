# Unreleased

* feat: Bind Docker networks to 127.0.0.1
* feat: Add IC options to network manifest (`ii`, `nns`, `subnets`, `artificial-delay-ms`)
* feat: Release for Windows
* feat: Add safety controls for `--set-controller` and `--remove-controller`
  * Warn and prompt for confirmation when removing yourself from controllers
  * Add `-f/--force` flag to skip confirmation prompts
* feat: Show `name` in `canister status` command
* feat: `icp canister metadata <canister> <metadata section>` now fetches metadata sections from specified canisters
* fix: Validate explicit canister paths and throw an error if `canister.yaml` is not found
* feat!: Rename the implicit "mainnet" network to "ic"
  * The corresponding environment "ic" is defined implicitly which can be overwritten by user configuration.
  * The `--mainnet` and `--ic` flags are removed. Use `-n/--network ic`, `-e/--environment ic` instead.
* feat: Allow overriding the implicit `local` network and environment.
* chore: get rid of `TCYCLES` mentions and replace them with `cycles`
* feat: Add `icp cycles transfer` as replacement for `icp token cycles transfer`
* chore!: remove support for `cycles` in `icp token`. Use `icp cycles` instead
* chore!: Change display format of token and cycles amounts
* feat: Token and cycles amounts now support new formats. Valid examples: `1_000`, `1k`, `1.5m`, `1_234.5b`, `4T`
* feat: Allow installing WASMs that are larger than 2MB
* feat: Add `icp identity account-id` command to display the ICP ledger account identifier
  * Supports `--of-principal` flag to convert a specific principal instead of the current identity
* feat: `icp token transfer` now accepts AccountIdentifier hex strings for ICP ledger transfers
  * Legacy ICP ledger transfers using AccountIdentifier are automatically used when a 64-character hex string is provided
  * AccountIdentifier format is only supported for the ICP ledger; other tokens require Principal format

# v0.1.0-beta.3

* feat: Remove requirement that the user install `icp-cli-network-launcher`, auto-install it on first use
* feat: Support keyring storage and password-protected encryption for identity keys (and make keyring the default)
* fix: Use EOP when upgrading motoko canisters
* feat: Network startup verbose output now requires `--debug` flag
* feat: Add `icp network status` command to display network information
  * Displays port, root key, and candid UI principal (if available)
  * Supports `--json` flag for JSON output
* feat: `icp deploy` now displays URLs to interact with the deployed canister(s)
* feat: Allow overriding the `local` network in the config file
  * This makes it more convenient to configure the default environment
* feat: Validate call argument against candid interface
  * The interface is fetched from canister metadata onchain
* feat: Accept an environment as argument for network commands
* feat: call argument building interactively using candid assist
* feat: specifying canister `init_args` in `icp.yaml`
* fix: overriding canister settings from the `canisters` section of `icp.yaml` with settings from the `environments` section now works as intended

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

