# icp-sync-plugin

Runs an icp-cli sync plugin, a WebAssembly component, on wasmtime. It is one
implementation of `icp-project`'s plugin-runner seam
(`icp_project::canister::sync::plugin::Run`).

Needs a multi-thread tokio runtime: a plugin's canister calls block the thread
the component runs on.

## Stability

This crate is part of [icp-cli](https://github.com/dfinity/icp-cli) and is
published so other tools can build on the same code the `icp` binary runs. Its
API is not stable: it is released together with the CLI, under the CLI's
version, and any release may change it. Depend on it with an exact version
(`=x.y.z`), and on the same version of the other `icp-*` crates.
