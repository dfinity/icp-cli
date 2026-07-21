//! Host-side canister facade.
//!
//! The canister *model* (`Settings`, `ControllerRef`, `resolve_controllers`,
//! log-visibility types, and the recipe `Resolve` interface) lives in
//! `icp_deploy_canister::canister` and is re-exported here. The build/sync/wasm
//! *executors* (which spawn processes, run wasmtime, and fetch over HTTP) stay
//! here.

pub use icp_deploy_canister::canister::{
    ControllerRef, LogVisibilityDef, LogVisibilitySimple, Settings, resolve_controllers,
};

pub mod build;
pub mod recipe;
pub mod sync;

mod script;
pub mod wasm;
