# icp-events

Typed progress events. An icp-cli operation never prints: it reports what it is
doing as events through a `Reporter`, and whoever drives the operation decides
how they look.

## Stability

This crate is part of [icp-cli](https://github.com/dfinity/icp-cli) and is
published so other tools can build on the same code the `icp` binary runs. Its
API is not stable: it is released together with the CLI, under the CLI's
version, and any release may change it. Depend on it with an exact version
(`=x.y.z`), and on the same version of the other `icp-*` crates.
