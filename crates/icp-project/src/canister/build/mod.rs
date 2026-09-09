use async_trait::async_trait;

use icp_events::StepReporter;
use snafu::prelude::*;

use std::sync::Arc;

use crate::canister::wasm;
use crate::manifest::canister::BuildStep;
use crate::prelude::*;

mod prebuilt;
mod script;

pub struct Params {
    pub path: PathBuf,
    pub output: PathBuf,
    pub environment: String,
}

#[derive(Debug, Snafu)]
pub enum BuildError {
    #[snafu(transparent)]
    Script { source: super::script::ScriptError },
    #[snafu(transparent)]
    Prebuilt { source: prebuilt::PrebuiltError },
}

#[async_trait]
pub trait Build: Sync + Send {
    async fn build(
        &self,
        step: &BuildStep,
        params: &Params,
        reporter: &StepReporter,
    ) -> Result<(), BuildError>;
}

/// Runs each build step where it has to be run: a script step in a subprocess,
/// a pre-built step by asking [`wasm::Fetch`] for the module.
pub struct Builder {
    wasm: Arc<dyn wasm::Fetch>,
}

impl Builder {
    pub fn new(wasm: Arc<dyn wasm::Fetch>) -> Self {
        Self { wasm }
    }
}

#[async_trait]
impl Build for Builder {
    async fn build(
        &self,
        step: &BuildStep,
        params: &Params,
        reporter: &StepReporter,
    ) -> Result<(), BuildError> {
        match step {
            BuildStep::Prebuilt(adapter) => {
                Ok(prebuilt::build(adapter, params, reporter, self.wasm.as_ref()).await?)
            }
            BuildStep::Script(adapter) => Ok(script::build(adapter, params, reporter).await?),
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
/// Unimplemented mock implementation of `Build`.
/// All methods panic with `unimplemented!()` when called.
pub struct UnimplementedMockBuilder;

#[cfg(any(test, feature = "test-util"))]
#[async_trait]
impl Build for UnimplementedMockBuilder {
    async fn build(
        &self,
        _step: &BuildStep,
        _params: &Params,
        _reporter: &StepReporter,
    ) -> Result<(), BuildError> {
        unimplemented!("UnimplementedMockBuilder::build")
    }
}
