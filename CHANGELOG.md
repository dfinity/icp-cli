# Unreleased

* feat: `icp identity import` now takes `--seed-curve`, for seed phrases for non-k256 keys.
* fix: `icp canister settings show` now outputs only the canister settings, consistent with the command name
* fix: Fail early when attempting to create an identity with an already existing name.
* fix: Find icp.yaml even from within a symlinked folder.

# v0.2.3

* feat: Add `--proxy` to `icp canister` subcommands and `icp deploy` to route management canister calls through a proxy canister
* feat: Add `--args`, `--args-file`, and `--args-format` flags to `icp deploy` to pass install arguments at the command line, overriding `init_args` in the manifest

# v0.2.2

Important: A network launcher more recent than v12.0.0-83c3f95e8c4ce28e02493df83df5f84a166451c0 is
required to use internet identity.

* feat: Many more commands support `--json` and `--quiet`.
* feat: When a local network is started internet identity is available at id.ai.localhost
* fix: Network would fail to start if a stale descriptor was present

# v0.2.1

* feat: icp-cli will now inform you if a new version is released. This can be disabled with `icp settings update-check`
* fix: Duplicate identities no longer cause an error when starting a network
* feat: Added support for creating canisters on cloud engine subnets. Note that local networks cannot yet create these subnets.
* feat: Upgrading canisters now stops them before the upgrade and starts them again afterwards
* feat: `icp canister logs` supports filtering by timestamp (`--since`, `--until`) and log index (`--since-index`, `--until-index`)
* feat: Support `log_memory_limit` canister setting in `icp canister settings update` and `icp canister settings sync`
* feat: Leaving off the method name parameter in `icp canister call` prompts you with an interactive list of methods
* fix: Correct templating of special HTML characters in recipes

# v0.2.0

* feat: Added a notification about new versions of the network
* feat: Added 'friendly name' domains for canisters - instead of `<frontend principal>.localhost` you can access `frontend.local.localhost`.
* feat: Added `bind` key to network gateway config to pick your network interface (previous documentation mentioned a `host` key, but it did not do anything)
* feat: check for Candid incompatibility when upgrading a canister
* feat: Add `bitcoind-addr` and `dogecoind-addr` options for managed networks to connect to Bitcoin and Dogecoin nodes
* feat: Init/call arg files now support raw binary without conversion to hex
* feat!: Remove argument type inference in init/call args in commands and manifest. Args are always assumed Candid, new parameters allow specifying other formats like hex, and alternate parameters are used to specify loading from a file.
* feat: Network gateway now supports a `domains` key
* feat: `icp identity export` now takes an `--encrypt` flag to avoid rendering the key in plaintext
* feat: Optionally split connected networks' `url` into `api-url` and `http-gateway-url`
* feat: Allow specifying a version of the network launcher to use
* feat: Support subaccounts and ICRC-1 IDs in `icp token`, `icp cycles`, and `icp identity account-id`
* feat!: Recipes are now specified `@registry/recipe@version`, the version component is required. The `latest` version is no longer assumed and the version tags will be removed soon.
* feat: Recipes and prebuilt canisters are now cached locally
* feat: `icp settings autocontainerize true`, always use a docker container for all networks
* feat: `icp canister migrate-id` - initiate canister ID migration across subnets
* feat: Install proxy canister when starting managed networks with all identities as controllers (or anonymous + default if more than 10 identities)
  * `icp network status` displays the proxy canister principal
* feat: `icp network status` display more information about networks
* feat: `icp canister logs` to display the current canister logs
  * use `--follow` to continuously poll for new logs. `--interval <n>` to poll every `n` seconds
* feat: Support `k`, `m`, `b`, `t` suffixes in `.yaml` files when specifying cycles amounts
* feat: Support `kb`, `kib`, `mb`, `mib`, `gb`, `gib` suffixes in `.yaml` files and CLI arguments when specifying memory amounts
* feat: Add an optional root-key argument to canister commands
* feat: `icp canister call` now supports `--output <mode>` with the following modes:
  * `auto` (default): Try decoding the response as Candid, then UTF-8, then fall back to hex.
  * `candid`: Parse as Candid and pretty-print; error if parsing fails.
  * `text`: Parse as UTF-8 text; error if invalid.
  * `hex`: Print raw response as hex.
* chore!: new passwords for identity encryption need to be at least 8 characters long
* feat: Anonymous usage telemetry — collects command name, arguments, duration, and outcome
  * Enabled by default; opt out with `icp settings telemetry false`, `DO_NOT_TRACK=1`, or `ICP_TELEMETRY_DISABLED=1`
  * Automatically disabled in CI environments (`CI` env var set)
  * `icp settings telemetry` to view or change the current setting

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

