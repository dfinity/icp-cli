//! Basic filesystem operations and JSON/YAML loaders with path-carrying errors.
//!
//! The base operations and the `json`/`yaml` loaders are defined in
//! `icp_deploy_canister::fs` and re-exported here; the `lock` submodule
//! (cross-process directory locking) is host-only and lives here.

pub use icp_deploy_canister::fs::*;

pub mod lock;
