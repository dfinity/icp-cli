# icp-canister-interfaces

Candid types, method names and canister ids for the Internet Computer system
canisters that icp-cli calls: the cycles ledger, the cycles minting canister,
the ICP ledger, the registry and others.

## Stability

This crate is part of [icp-cli](https://github.com/dfinity/icp-cli) and is
published so other tools can build on the same code the `icp` binary runs. Its
API is not stable: it is released together with the CLI, under the CLI's
version, and any release may change it. Depend on it with an exact version
(`=x.y.z`), and on the same version of the other `icp-*` crates.
