# icp-sync-plugin

Runs an icp-cli sync plugin, a WebAssembly component, on wasmtime. It is one
implementation of `icp-project`'s plugin-runner seam
(`icp_project::canister::sync::plugin::Run`).

Needs a multi-thread tokio runtime: a plugin's canister calls block the thread
the component runs on.

## Stability

This crate is part of [icp-cli](https://github.com/dfinity/icp-cli) and is
published so other tools can build on the same code the `icp` binary runs. Its
API is the CLI's internals, and is not stable: a new minor version comes out
with every icp-cli release, and any of them may change it. The other `icp-*`
crates are released alongside under the same version number; use the same
version of each.
