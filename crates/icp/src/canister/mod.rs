//! Host-side canister facade.
//!
//! The canister *model* (`Settings`, `ControllerRef`, `resolve_controllers`,
//! log-visibility types, the `RemoteResourceResolve` interface) lives in
//! `icp_deploy_canister::canister` and is re-exported here. The build/wasm/recipe
//! *executors* (which spawn processes, fetch over HTTP, and cache) stay here.

pub use icp_deploy_canister::canister::{
    ControllerRef, LogVisibilityDef, LogVisibilitySimple, Settings, resolve_controllers,
};

pub mod build;
pub mod recipe;
pub mod script;
pub mod wasm;
