# icp-project

The icp-cli project model: loading and consolidating manifests, canisters and
environments, and the operations that build, install, sync and deploy them.

What the crate needs from the machine it runs on it asks for through traits
(files, canister calls, network access, running a build step), so a host other
than the CLI can supply its own. The default-on `host` feature adds
implementations backed by the local filesystem and subprocesses; without it the
crate builds for `wasm32-unknown-unknown`.

## Stability

This crate is part of [icp-cli](https://github.com/dfinity/icp-cli) and is
published so other tools can build on the same code the `icp` binary runs. Its
API is the CLI's internals, and is not stable: a new minor version comes out
with every icp-cli release, and any of them may change it. The other `icp-*`
crates are released alongside under the same version number; use the same
version of each.
