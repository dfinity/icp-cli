//! Host-side canister facade.
//!
//! The canister *model* (`Settings`, `ControllerRef`, `resolve_controllers`,
//! log-visibility types, and the `RemoteResourceResolve` interface) lives in
//! `icp_deploy_canister::canister` and is re-exported here. The build/sync/wasm
//! *executors* (which spawn processes, run wasmtime, and fetch over HTTP) stay
//! here.

use icp_deploy_canister::sync_exec::StepProgress;
use tokio::sync::mpsc::Sender;

pub use icp_deploy_canister::canister::{
    ControllerRef, LogVisibilityDef, LogVisibilitySimple, ManifestEnvVar, ManifestSettings,
    Settings, resolve_controllers,
};

pub mod build;
pub mod recipe;
pub mod sync;

mod script;
pub mod wasm;

/// Adapts a streamed-output channel to the library's [`StepProgress`] line sink,
/// so host code that already owns a channel can hand one to the library's IO
/// traits.
pub struct ChannelProgress(pub Sender<String>);

impl StepProgress for ChannelProgress {
    fn line(&self, line: String) {
        // Status lines are advisory: drop them rather than block the caller if
        // the display has fallen behind.
        let _ = self.0.try_send(line);
    }
}

impl ChannelProgress {
    /// Wrap an optional channel, as the library's IO traits take it.
    pub fn wrap(stdio: Option<&Sender<String>>) -> Option<Self> {
        stdio.cloned().map(Self)
    }

    /// Borrow as the trait object the library's IO traits take.
    pub fn as_dyn(this: Option<&Self>) -> Option<&dyn StepProgress> {
        this.map(|p| p as &dyn StepProgress)
    }
}
