use std::{fmt, sync::Arc};

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::{
    canister::build,
    manifest::adapter::{prebuilt, script},
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
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
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
                Step::Script(v) => format!("script {v}"),
                Step::Prebuilt(v) => format!("pre-built {v}"),
            }
        )
    }
}

/// Describes how the canister should be built into WebAssembly,
/// including the adapters and build steps responsible for the build.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
pub struct Steps {
    pub steps: Vec<Step>,
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Build: Sync + Send {
    async fn build(&self, step: build::Step) -> Result<(), BuildError>;
}

pub struct Builder {
    pub prebuilt: Arc<dyn Build>,
    pub script: Arc<dyn Build>,
}

#[async_trait]
impl Build for Builder {
    async fn build(&self, step: build::Step) -> Result<(), BuildError> {
        match step {
            build::Step::Prebuilt(_) => self.prebuilt.build(step).await,
            build::Step::Script(_) => self.script.build(step).await,
        }
    }
}
