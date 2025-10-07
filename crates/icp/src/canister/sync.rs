use std::{fmt, sync::Arc};

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::mpsc::Sender;

use crate::{
    canister::sync,
    manifest::adapter::{assets, script},
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
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
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
#[derive(Clone, Debug, Default, Deserialize, PartialEq, JsonSchema)]
pub struct Steps {
    pub steps: Vec<Step>,
}

#[derive(Debug, thiserror::Error)]
pub enum SynchronizeError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Synchronize: Sync + Send {
    async fn sync(
        &self,
        step: sync::Step,
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
        step: sync::Step,
        stdio: Option<Sender<String>>,
    ) -> Result<(), SynchronizeError> {
        match step {
            sync::Step::Assets(_) => self.assets.sync(step, stdio).await,
            sync::Step::Script(_) => self.script.sync(step, stdio).await,
        }
    }
}
