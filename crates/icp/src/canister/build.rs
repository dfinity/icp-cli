use std::{fmt, sync::Arc};

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{Sender, error::SendError};

use crate::{
    canister::{build, script::ScriptError},
    manifest::{
        adapter::{prebuilt, script},
        serde_helpers::non_empty_vec,
    },
    prelude::*,
};

/// Identifies the type of adapter used to build the canister,
/// along with its configuration.
///
/// The adapter type is specified via the `type` field in the YAML file.
/// For example:
///
/// ```yaml
/// type: rust
/// package: my_canister
/// ```
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Step {
    /// Represents a canister built using a custom script or command.
    /// This variant allows for flexible build processes defined by the user.
    Script(script::Adapter),

    /// Represents a pre-built canister.
    /// This variant allows for retrieving a canister WASM from various sources.
    #[serde(rename = "pre-built")]
    Prebuilt(prebuilt::Adapter),
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Step::Script(v) => format!("(script)\n{v}"),
                Step::Prebuilt(v) => format!("(pre-built)\n{v}"),
            }
        )
    }
}

/// Describes how the canister should be built into WebAssembly,
/// including the adapters and build steps responsible for the build.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct Steps {
    #[serde(deserialize_with = "non_empty_vec")]
    pub steps: Vec<Step>,
}

pub struct Params {
    pub path: PathBuf,
    pub output: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error(transparent)]
    Script(#[from] ScriptError),

    #[error("failed to send build output")]
    SendOutput(#[from] SendError<String>),

    #[error("failed to join futures")]
    JoinError(#[from] tokio::task::JoinError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Build: Sync + Send {
    async fn build(
        &self,
        step: &build::Step,
        params: &Params,
        stdio: Option<Sender<String>>,
    ) -> Result<(), BuildError>;
}

pub struct Builder {
    pub prebuilt: Arc<dyn Build>,
    pub script: Arc<dyn Build>,
}

#[async_trait]
impl Build for Builder {
    async fn build(
        &self,
        step: &build::Step,
        params: &Params,
        stdio: Option<Sender<String>>,
    ) -> Result<(), BuildError> {
        match step {
            build::Step::Prebuilt(_) => self.prebuilt.build(step, params, stdio).await,
            build::Step::Script(_) => self.script.build(step, params, stdio).await,
        }
    }
}

// ============================================================================
// Test utilities
// ============================================================================

#[cfg(any(test, feature = "test-utils"))]
pub mod test {
    use async_trait::async_trait;
    use tokio::sync::mpsc::Sender;

    use super::*;

    /// Mock builder for testing.
    ///
    /// Can be configured to return success or a specific error message.
    pub struct MockBuilder {
        error_msg: Option<String>,
    }

    impl MockBuilder {
        pub fn new() -> Self {
            Self { error_msg: None }
        }

        pub fn with_error(msg: impl Into<String>) -> Self {
            Self {
                error_msg: Some(msg.into()),
            }
        }
    }

    impl Default for MockBuilder {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl Build for MockBuilder {
        async fn build(
            &self,
            _step: &build::Step,
            _params: &Params,
            _stdio: Option<Sender<String>>,
        ) -> Result<(), BuildError> {
            match &self.error_msg {
                None => Ok(()),
                Some(msg) => Err(BuildError::Unexpected(anyhow::anyhow!("{}", msg))),
            }
        }
    }
}
