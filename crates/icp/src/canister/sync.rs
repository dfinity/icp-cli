use std::{fmt, sync::Arc};

use async_trait::async_trait;
use candid::Principal;
use ic_agent::Agent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::Sender;

use crate::{
    canister::{script::ScriptError, sync},
    manifest::adapter::{assets, script},
    prelude::*,
};

/// Identifies the type of adapter used to sync the canister,
/// along with its configuration.
///
/// The adapter type is specified via the `type` field in the YAML file.
/// For example:
///
/// ```yaml
/// type: script
/// command: echo "synchronizing canister"
/// ```
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Step {
    /// Represents a canister synced using a custom script or command.
    /// This variant allows for flexible sync processes defined by the user.
    Script(script::Adapter),

    /// Represents syncing of an assets canister
    Assets(assets::Adapter),
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Step::Script(v) => format!("script {v}"),
                Step::Assets(v) => format!("assets {v}"),
            }
        )
    }
}

/// Describes how the canister should be built into WebAssembly,
/// including the adapters and build steps responsible for the build.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Steps {
    pub steps: Vec<Step>,
}

pub struct Params {
    pub path: PathBuf,
    pub cid: Principal,
}

#[derive(Debug, thiserror::Error)]
pub enum SynchronizeError {
    #[error(transparent)]
    Script(#[from] ScriptError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Synchronize: Sync + Send {
    async fn sync(
        &self,
        step: &sync::Step,
        params: &Params,
        agent: &Agent,
        stdio: Option<Sender<String>>,
    ) -> Result<(), SynchronizeError>;
}

pub struct Syncer {
    pub assets: Arc<dyn Synchronize>,
    pub script: Arc<dyn Synchronize>,
}

#[async_trait]
impl Synchronize for Syncer {
    async fn sync(
        &self,
        step: &sync::Step,
        params: &Params,
        agent: &Agent,
        stdio: Option<Sender<String>>,
    ) -> Result<(), SynchronizeError> {
        match step {
            sync::Step::Assets(_) => self.assets.sync(step, params, agent, stdio).await,
            sync::Step::Script(_) => self.script.sync(step, params, agent, stdio).await,
        }
    }
}

#[cfg(test)]
/// Unimplemented mock implementation of `Synchronize` for testing purposes.
///
/// All methods panic with `unimplemented!()` when called.
/// This is useful for tests that need to construct a context but don't
/// actually use the sync functionality.
pub struct UnimplementedMockSyncer;

#[cfg(test)]
#[async_trait]
impl Synchronize for UnimplementedMockSyncer {
    async fn sync(
        &self,
        _step: &sync::Step,
        _params: &Params,
        _agent: &Agent,
        _stdio: Option<Sender<String>>,
    ) -> Result<(), SynchronizeError> {
        unimplemented!("UnimplementedMockSyncer::sync")
    }
}
