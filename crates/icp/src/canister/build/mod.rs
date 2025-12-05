use std::{fmt, sync::Arc};

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::{
    canister::{build, prebuilt::PrebuiltError, script::ScriptError},
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
/// type: script
/// command: do_something.sh
/// ```
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum BuildStep {
    /// Represents a canister built using a custom script or command.
    /// This variant allows for flexible build processes defined by the user.
    Script(script::Adapter),

    /// Represents a pre-built canister.
    /// This variant allows for retrieving a canister WASM from various sources.
    #[serde(rename = "pre-built")]
    Prebuilt(prebuilt::Adapter),
}

impl fmt::Display for BuildStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                BuildStep::Script(v) => format!("(script)\n{v}"),
                BuildStep::Prebuilt(v) => format!("(pre-built)\n{v}"),
            }
        )
    }
}

/// Describes how the canister should be built into WebAssembly,
/// including the adapters and build steps responsible for the build.
#[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema, Serialize)]
pub struct BuildSteps {
    #[serde(deserialize_with = "non_empty_vec")]
    pub steps: Vec<BuildStep>,
}

pub struct Params {
    pub path: PathBuf,
    pub output: PathBuf,
}

#[derive(Debug, Snafu)]
pub enum BuildError {
    #[snafu(transparent)]
    Script { source: ScriptError },
    #[snafu(transparent)]
    Prebuilt { source: PrebuiltError },
}

#[async_trait]
pub trait Build: Sync + Send {
    async fn build(
        &self,
        step: &build::BuildStep,
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
        step: &build::BuildStep,
        params: &Params,
        stdio: Option<Sender<String>>,
    ) -> Result<(), BuildError> {
        match step {
            build::BuildStep::Prebuilt(_) => self.prebuilt.build(step, params, stdio).await,
            build::BuildStep::Script(_) => self.script.build(step, params, stdio).await,
        }
    }
}

#[cfg(test)]
/// Unimplemented mock implementation of `Build`.
/// All methods panic with `unimplemented!()` when called.
pub struct UnimplementedMockBuilder;

#[cfg(test)]
#[async_trait]
impl Build for UnimplementedMockBuilder {
    async fn build(
        &self,
        _step: &build::BuildStep,
        _params: &Params,
        _stdio: Option<Sender<String>>,
    ) -> Result<(), BuildError> {
        unimplemented!("UnimplementedMockBuilder::build")
    }
}
