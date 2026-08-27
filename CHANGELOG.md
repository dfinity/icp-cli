<!--
Convention: changes to experimental features live in a dedicated
`## Experimental` subsection under each version. Experimental features
may receive breaking changes between releases without a major version
bump. Currently experimental: project bundling, project dependencies,
air-gapped signing
-->

# Unreleased

* feat: a canister can now declare `upgrade_args` alongside `init_args`, in its own manifest and as a per-canister environment override. It is passed when `icp deploy` upgrades the canister, where `init_args` is passed when it installs or reinstalls it. It takes exactly the forms `init_args` does (inline Candid string, or `{ value | path, format }`), and paths resolve against the canister's own directory the same way. A canister that declares no `upgrade_args` is upgraded with its `init_args`, as before, and `--args` / `--args-file` still override whichever applies.
* feat: `script` build steps now receive `ICP_CLI_ENVIRONMENT`, the name of the environment the canisters are being built for, so a build can vary by environment the way a sync step already could.
* feat: `icp completions <SHELL>` prints a shell completion script for `bash`, `zsh`, `fish`, `powershell`, or `elvish` to stdout. See the [installation guide](docs/guides/installation.md#shell-completions) for where to put it.
* fix: `icp canister logs` output formats are corrected. `--json` now emits machine-readable JSON and the default emits the human-readable lines (the two were swapped), and `--follow --json` emits newline-delimited JSON, one record per line, streamed as each record arrives. This is breaking for scripts: parsing the default output as JSON now requires `--json`, and consumers of `--follow --json` must read one JSON object per line.
* fix: `icp canister status` again falls back on the publicly readable state-tree information when the caller may not read the status. Replicas now reject those calls with `IC0542`, which the fallback did not recognise, so the command failed with `Error looking up canister <id>` instead of printing the controllers and module hash. `IC0541`, returned on subnets with administrators, is now recognised too, and the fallback no longer depends on whether the rejection arrives certified or uncertified.

## Experimental

* feat(signing): a canister call can now be signed on one machine and submitted from another, restoring what `dfx canister sign` / `dfx canister send` covered. `icp canister call --sign-only <FILE>` composes and signs a call and writes it to a JSON file instead of submitting it; `icp message send <FILE>` submits that file and prints the reply. So a machine that holds the key needs no network, and the machine with the network needs no key — it never resolves an identity at all. `-` writes to stdout and reads from stdin respectively.
  * Nothing is fetched while signing: the Candid interface comes from `--candid` or from the canister's local build artifact rather than from the canister itself, and `--root-key` must name a key (`mainnet` or a hex-encoded key) rather than `fetch`. `--proxy` is not supported.
  * `icp message send` shows what the message contains — sender, canister, method, decoded argument, window, and destination — and asks before submitting. `--yes` skips the prompt, and a non-TTY proceeds without one so a scripted courier works. `--dry-run` prints the same summary and stops without touching the network at all, which makes it the file-inspection command. `--candid`, `--output` and `--json` render the reply exactly as `icp canister call` does. Where the message is submitted comes from the file alone — there is no `--network` override, so no environment variable can silently redirect a signed message; a courier who has to change it edits the file's `network` field, which is unauthenticated in either case.
  * If sending fails after the message may already have gone out, re-run `icp message send` **on the same file**: the request id is a hash of the signed content, so resubmitting the identical message is de-duplicated by the IC and cannot execute twice. Signing again produces a new expiry, hence a different request id, which is *not* de-duplicated — for a transfer, a double spend. Every post-submission failure says so.
  * `--valid-from <WHEN>` places the message's submission window, as a duration from now (`55m`, `2h`) or an RFC 3339 timestamp; it defaults to now. The window is always five minutes wide, because the IC rejects an ingress message whose expiry is further ahead than that — so this places the window rather than sizing it. Note that this is not dfx's `--expire-after`, which names the window's *end* and leaves you to subtract the five minutes yourself.
  * The file records the signed envelope, where to submit it, a tagged canister-or-subnet destination, the Candid interface, and a human-readable summary of what was signed. An update also carries a pre-signed `request_status` read, so the submitting machine can await the outcome with no key of its own; it shares the call's expiry, so both live in the same window.

# v1.3.0

* feat: a canister environment variable's value can now be read from a file, by writing `var: { path: <file> }` in place of `var: value`. The path resolves against the canister's directory — including in an environment override, matching `init_args` — and surrounding whitespace is trimmed off the file's contents. The file is read when the project is loaded, so a missing file fails before anything is deployed. `icp project bundle` writes the value into the bundled manifest inline, rejecting a file outside the project as it does for other manifest file references.
* fix: `icp network start` now explains why a Docker-based network failed to come up. A container that exited before the network was ready was reported as `failed to watch docker container <id> for exit` with an empty cause, discarding the actual reason (e.g. the gateway port already being taken); the container's output is now attached to the error.
* feat: Docker-based networks now show the launcher's output like non-containerized ones do. In the foreground the container's stdout and stderr are streamed to your terminal as it runs; in background mode `icp network start` prints the `docker logs -f <container-id>` command to follow it. Previously container output was never shown at all — which on Windows, where the launcher always runs in a container, meant `icp network start` was silent.
* fix: a network launcher running in a container (`icp settings autocontainerize true`, and always on Windows) now receives `--verbose` when `icp -d` is used, matching the non-containerized launcher.

## Experimental

* feat(bundle): `icp project bundle` takes `-e/--environment`, naming the environment its canisters are built for — it reaches build scripts as `ICP_CLI_ENVIRONMENT`. It defaults to `ic`, unlike the rest of the CLI, because a bundle is built to be deployed somewhere else; `ICP_ENVIRONMENT` overrides that default as it does elsewhere. Which canisters are bundled is unaffected.
* feat(bundle): `icp project bundle` now works on projects that declare `dependencies:`, which it previously refused outright. The bundle mirrors the workspace instead of flattening it: the root project's `icp.yaml` sits at the archive root, each dependency instance gets its own `icp.yaml` at the directory it occupies in the workspace, and the `dependencies:` declarations are preserved, each pointing at the directory its dependency occupies in the archive (the same path a plainly vendored layout already used). A shared (diamond) dependency is still a single instance, canister names stay as each project wrote them, and canister discovery (`PUBLIC_CANISTER_ID:<alias>:<canister>`) works in the extracted bundle exactly as it did in the source workspace.
  * Every dependency must resolve to a directory inside the workspace root; one that resolves outside it (including through a symlink) is rejected, because the archive could not contain it. As a result, a vendored member that depends on a sibling cannot be bundled as a standalone project (e.g. via `ICP_PROJECT_ROOT`) — bundle the workspace root instead.
  * Projects with script sync steps still cannot be bundled, and the restriction now covers every project in the workspace.

# v1.2.0

* feat(sync-plugin): the compute-time limit for `plugin` sync steps is now configurable via the `ICP_CLI_PLUGIN_COMPUTE_LIMIT_SECS` environment variable (default `60`). Raise it for compute-heavy plugins (e.g. brotli-compressing a large asset bundle) that legitimately exceed the default, especially on slower CI runners. The limit-exceeded error now names the variable and the current limit, and a malformed value is rejected rather than silently ignored.
* feat: `icp canister link` assigns an existing canister principal to a project canister
* feat: `icp canister create --with-icp` (not supported in `icp deploy`) uses the CMC to create canisters. Only needed for deploying to restricted system subnets.
* feat: `icp deploy --no-create` will error if any canisters do not exist, rather than creating them.
* fix: `icp deploy` now prints the frontend URL for any canister that exposes an `http_request` endpoint. Previously a canister whose `http_request` signature differed from a hard-coded shape (e.g. some certified-asset canisters) was misdetected and shown a Candid UI URL instead of its site URL. Deploy URLs are also now grouped by kind (frontends vs. Candid UI) instead of interleaved.

# v1.1.0

* feat: `icp token [TOKEN|LEDGER_ID] approve <AMOUNT> <SPENDER>` grants an ICRC-2 allowance, letting a spender transfer tokens on your behalf. Supports `--from-subaccount` (the account debited for the allowance), `--spender-subaccount`, and an optional `--expires-in <DURATION>` (e.g. `24h`, `30d`) to auto-expire the allowance.
* feat: `icp token [TOKEN|LEDGER_ID] allowance <SPENDER>` displays the ICRC-2 allowance granted to a spender. Supports `--subaccount`, `--spender-subaccount`, and `--of-principal` to inspect any account.
* feat: `icp canister delete` will now send the canister's remaining cycles to the caller
* feat: Connected networks now take an explicit `root-key`, which accepts a hex-encoded key or one of two new values:
  * `mainnet`: use the canonical IC mainnet root key — handy for reaching mainnet through a custom boundary node without repeating the literal.
  * `fetch`: fetch the key from the network on each use. This is trust-on-first-use and does *not* verify the key's provenance, so it's meant only for testnets you or someone you trust operate; `icp` prints a warning whenever it fetches.
  * `network status` reports where the key came from — a `root_key_source` field in `--json`, and a `(fetched - unverified, trust-on-first-use)` label in text output.
  * `root-key` is now required for connected networks (previously optional, silently defaulting to the mainnet key). This is technically breaking, but most working projects are unaffected: a non-mainnet connected network already needed an explicit key, so in practice only a mainnet-via-custom-URL network needs to add `root-key: mainnet`. The built-in `ic` network is unchanged.

## Experimental

* feat: Projects can now depend on other `icp` projects vendored into them (e.g. as git submodules) via a top-level `dependencies:` block in `icp.yaml`.
  * `icp deploy` deploys the dependency alongside your project and injects its canister IDs.
  * Running `icp` from inside a vendored sub-project resolves up to the workspace root, so the whole workspace shares one network and one set of canister IDs.
  * Canister names and dependency aliases must now contain only ASCII letters, digits, `_`, and `-`, with `:` reserved as the dependency namespace separator. Names using other characters are now rejected.
  * See the [Project Dependencies](docs/concepts/project-dependencies.md) concept guide for details.

# v1.0.2

* feat(sync-plugin): `plugin` sync steps now reject any `dirs`/`files` entry that is, or traverses, a symlink. Together with the existing relative-path and `..` checks, this keeps a declared path from resolving to a target outside the canister directory. The restriction may be relaxed in a future release if a safe use case emerges.

## Experimental

* fix(bundle): path validation use parent-dir analysis without canonicalize.

# v1.0.1

* feat: `icp identity import` can now be used with a `--delegation` flag to import a delegated identity. This is most useful for containers or other internal-only delegations; for anything involving a network, `icp identity delegation request` remains the recommended way to work with delegations.

# v1.0.0

* feat: The default gateway domain is now `icp.net`, not `icp0.io`.
* feat: Password-protected identities now only need your password once per session. The session length defaults to 5 minutes and can be changed with `icp settings session-length <DURATION>` (e.g. `30m`, `1h`) or turned off with `icp settings session-length disabled`. You can also explicitly create or refresh a session with `icp identity reauth <NAME> [--duration <DURATION>]`.
* feat!: Remove `--set-controller` and replace with a new flag `--remove-all-controllers`. For the old behavior, combine this flag with `--add-controller`

# v0.3.2

* feat: `icp canister call` now accepts `--candid <PATH>` to load the canister's Candid interface from a local `.did` file instead of fetching it from the network. The supplied interface drives method selection, argument building, and response decoding.

# v0.3.1

* fix: Account seeding in new networks is now done via transfer instead of mint. This should eliminate minting ratelimit errors for users with a lot of local identities.

# v0.3.0

* feat: `icp identity link web` now lets you sign in with web-based identities, especially Internet Identity. You can use your NNS-UI or Oisy principals locally with `--app nns.ic0.app` or `--app oisy.com`. When the session expires, sign in again with `icp identity reauth`.
* BREAKING: removed the `assets` sync step type (`type: assets`). Asset uploading is no longer built into icp-cli — use a `script` or `plugin` sync step instead (for example, a recipe that provides a sync plugin). A manifest that still uses `type: assets` now fails to load with a message explaining the change.
* feat: Recipe templates can now use `{{_.canister.name}}` to reference the canister's name from `icp.yaml` without repeating it in the `configuration:` block. The `_` namespace is reserved and cannot be overridden by user-provided configuration.

# v0.2.7

* feat: `script` sync steps now receive `ICP_CLI_ENVIRONMENT`, `ICP_CLI_NETWORK`, `ICP_CLI_CID` (the current canister's principal), and `ICP_CLI_CID_<NAME>` (every canister's principal) as environment variables.
* fix: `icp canister call` with both `--json` and `-o hex` no longer prints both kinds of output at once.
* fix: `icp` no longer picks up a stale inherited `$PWD` when launched as a subprocess via `chdir(2)` + `execve` (e.g. from a test harness). The logical `$PWD` path is now validated against `getcwd()` by inode before use, preserving symlink-aware project root discovery while ignoring stale values.

## Experimental

* feat(sync-plugin): Plugins can now surface messages that persist after the step completes. Anything the plugin writes to stderr (e.g. `eprintln!` in Rust) is streamed live in the rolling step view AND printed under the canister name once the step ends; stdout remains transient. The `exec()` return signature has changed from `result<option<string>, string>` to `result<_, string>` — plugins that returned a summary string should `eprintln!` it instead.

# v0.2.6

* feat: `icp token/cycles balance` now accept `--of-principal`
* fix: The local wasm cache has moved from `.icp/cache/canisters/` to `.icp/cache/wasms/`. Existing cached files will be re-downloaded automatically on the next run.
* fix: `icp canister call` now serializes arguments built via the interactive Candid assist prompt against the method's declared signature, matching the behavior of arguments passed on the command line. Previously, narrower values (e.g. a variant case from a multi-case variant) were encoded with a type table inferred only from the value, which the target canister rejected with errors like "Variant index N larger than length 1".

## Experimental

* feat(sync-plugin): Canister manifests now support a `plugin` sync step type. Plugins are WebAssembly components that run in a sandboxed environment and can drive arbitrary post-deployment logic against the canister being synced. See `crates/icp-sync-plugin/DESIGN.md` for details.
* feat(sync-plugin): `icp sync` now accepts `--proxy` to route sync plugin calls to the target canister through a proxy canister.

# v0.2.5

* feat: `icp new --init` no longer requires specifying a project name. If non is provided, the containing folder's name is used as the project name
* fix: `icp canister call --json` no longer produces blank output.

# v0.2.4

* feat: `icp identity delegation request/sign/use` now permit creating and importing identity delegations
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

