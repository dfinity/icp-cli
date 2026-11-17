# icp-events

Typed progress events. An icp-cli operation never prints: it reports what it is
doing as events through a `Reporter`, and whoever drives the operation decides
how they look.

## Stability

This crate is part of [icp-cli](https://github.com/dfinity/icp-cli) and is
published so other tools can build on the same code the `icp` binary runs. Its
API is the CLI's internals, and is not stable: a new minor version comes out
with every icp-cli release, and any of them may change it. The other `icp-*`
crates are released alongside under the same version number; use the same
version of each.
