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

// ============================================================================
// Test utilities
// ============================================================================

#[cfg(any(test, feature = "test-utils"))]
pub mod test {
    use async_trait::async_trait;
    use ic_agent::Agent;
    use tokio::sync::mpsc::Sender;

    use super::*;

    /// Mock synchronizer for testing.
    ///
    /// Can be configured to return success or a specific error message.
    pub struct MockSynchronizer {
        error_msg: Option<String>,
    }

    impl MockSynchronizer {
        pub fn new() -> Self {
            Self { error_msg: None }
        }

        pub fn with_error(msg: impl Into<String>) -> Self {
            Self {
                error_msg: Some(msg.into()),
            }
        }
    }

    impl Default for MockSynchronizer {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl Synchronize for MockSynchronizer {
        async fn sync(
            &self,
            _step: &sync::Step,
            _params: &Params,
            _agent: &Agent,
            _stdio: Option<Sender<String>>,
        ) -> Result<(), SynchronizeError> {
            match &self.error_msg {
                None => Ok(()),
                Some(msg) => Err(SynchronizeError::Unexpected(anyhow::anyhow!("{}", msg))),
            }
        }
    }
}
