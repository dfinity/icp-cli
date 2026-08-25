//! Host-side canister facade.
//!
//! The canister *model* (`Settings`, `ControllerRef`, `resolve_controllers`,
//! the visibility types, and the `RemoteResourceResolve` interface) lives in
//! `icp_deploy_canister::canister` and is re-exported here. The build/sync/wasm
//! *executors* (which spawn processes, run wasmtime, and fetch over HTTP) stay
//! here.

use icp_deploy_canister::sync_exec::StepProgress;
use icp_events::StepReporter;

pub use icp_deploy_canister::canister::{
    ControllerRef, LogVisibilityDef, ManifestEnvVar, ManifestSettings, Settings,
    SnapshotVisibilityDef, StatusVisibilityDef, Visibility, resolve_controllers,
};

pub mod build;
pub mod recipe;
pub mod sync;

mod script;
pub mod wasm;

/// Adapts a step reporter to the library's [`StepProgress`] line sink, so host
/// code that reports on the event stream can hand one to the library's IO
/// traits.
pub struct ReporterProgress<'a>(pub &'a StepReporter);

impl StepProgress for ReporterProgress<'_> {
    fn line(&self, line: String) {
        self.0.info(line);
    }
}
