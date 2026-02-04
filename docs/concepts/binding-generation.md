# Binding Generation

How to generate type-safe client code for calling canisters.

## What Are Bindings?

Bindings are generated code that provides type-safe access to canister methods. They're created from Candid interface files (`.did`), which define a canister's public API.

icp-cli focuses on deployment — use these dedicated tools to generate bindings:

## Candid Interface Files

Candid is the interface description language for the Internet Computer. A `.did` file defines the public methods and types a canister exposes — it's the contract between a canister and its callers.

`.did` files can be:
- **Manually authored** — Recommended for stable APIs where backward compatibility matters
- **Generated from code** — Convenient during development, but review before publishing

For Candid syntax and best practices, see the [Candid specification](https://github.com/dfinity/candid/blob/master/spec/Candid.md).

## Generating Client Bindings

| Language | Tool | Documentation |
|----------|------|---------------|
| TypeScript/JavaScript | `@icp-sdk/bindgen` | [npmjs.com/@icp-sdk/bindgen](https://www.npmjs.com/package/@icp-sdk/bindgen) |
| Rust | `candid` crate | [docs.rs/candid](https://docs.rs/candid) |
| Other languages | `didc` CLI | [github.com/dfinity/candid](https://github.com/dfinity/candid) |

### TypeScript/JavaScript

Use `@icp-sdk/bindgen` to generate TypeScript bindings from Candid files. See the [@icp-sdk/bindgen documentation](https://www.npmjs.com/package/@icp-sdk/bindgen) for usage and build tool integration.

### Rust

The `candid` crate provides Candid serialization and code generation macros. See the [candid crate documentation](https://docs.rs/candid).

### Other Languages

The `didc` CLI generates bindings for various languages. See the [Candid repository](https://github.com/dfinity/candid) for available targets.

## See Also

- [Canister Discovery](canister-discovery.md) — How canisters find each other's IDs
- [Local Development](../guides/local-development.md) — Development workflow

[Browse all documentation →](../index.md)
