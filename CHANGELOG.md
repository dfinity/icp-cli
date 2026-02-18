# Unreleased

* feat: Init/call arg files now support raw binary without conversion to hex
* feat!: Remove argument type inference in init/call args in commands and manifest. Args are always assumed Candid, new parameters allow specifying other formats like hex, and alternate parameters are used to specify loading from a file.
* feat: Optionally split connected networks' `url` into `api-url` and `http-gateway-url`
* feat: Allow specifying a version of the network launcher to use
* feat: Support subaccounts and ICRC-1 IDs in `icp token`, `icp cycles`, and `icp identity account-id`
* feat!: Recipes are now specified `@registry/recipe@version`, the version component is required. The `latest` version is no longer assumed and the version tags will be removed soon.
* feat: Recipes and prebuilt canisters are now cached locally
* feat: `icp settings autocontainerize true`, always use a docker container for all networks
* feat: `icp canister migrate-id` - initiate canister ID migration across subnets
* feat: install proxy canister when starting managed networks with all identities as controllers (or anonymous + default if more than 10 identities)
  * `icp network status` displays the proxy canister principal
* feat: `icp network status` display more information about networks

# v0.1.0

* feat: `icp canister snapshot` - create, delete, restore, list, download, and upload canister snapshots
* feat: `icp canister call` now supports `--proxy` flag to route calls through a proxy canister
  * Use `--proxy <CANISTER_ID>` to forward the call through a proxy canister's `proxy` method
  * Use `--cycles <AMOUNT>` to specify cycles to forward with the proxied call (defaults to 0)

# v0.1.0-beta.6

* feat: `icp identity export` to print the PEM file for the identity

# v0.1.0-beta.5

* fix: Fix error when loading network descriptors from v0.1.0-beta.3
* feat: `icp identity delete` and `icp identity rename`

# v0.1.0-beta.4

* fix: More reliably detect occupied ports' project locations across containers and backgrounded networks
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
* feat: `icp canister call` can now take arguments in hex
* feat: allow specifying paths to files that contain canister arguments:
  * in `icp canister call <canister> <function> <argument>` the argument can now point to a file that contains hex or Candid
  * in `icp canister install <canister> <argument>` the argument can now point to a file that contains hex or Candid
  * in `icp.yaml`, a canister's `install_args` field can now point to a file that contains hex or Candid

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

