use async_trait::async_trait;

use icp_events::StepReporter;
use snafu::prelude::*;

#[cfg(feature = "host")]
use std::sync::Arc;

#[cfg(feature = "host")]
use crate::canister::wasm;
use crate::manifest::canister::BuildStep;
use crate::prelude::*;

// Both halves of the one implementation below, which is host-side.
#[cfg(feature = "host")]
mod prebuilt;
#[cfg(feature = "host")]
mod script;

pub struct Params {
    pub path: PathBuf,
    pub output: PathBuf,
    pub environment: String,
}

#[derive(Debug, Snafu)]
pub enum BuildError {
    #[cfg(feature = "host")]
    #[snafu(transparent)]
    Script { source: super::script::ScriptError },
    #[cfg(feature = "host")]
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
///
/// Only a host can run the script half, so this whole implementation is
/// host-side; somewhere without subprocesses supplies its own [`Build`].
#[cfg(feature = "host")]
pub struct Builder {
    wasm: Arc<dyn wasm::Fetch>,
    files: Arc<dyn crate::files::FileSystem>,
}

#[cfg(feature = "host")]
impl Builder {
    pub fn new(wasm: Arc<dyn wasm::Fetch>, files: Arc<dyn crate::files::FileSystem>) -> Self {
        Self { wasm, files }
    }
}

#[cfg(feature = "host")]
#[async_trait]
impl Build for Builder {
    async fn build(
        &self,
        step: &BuildStep,
        params: &Params,
        reporter: &StepReporter,
    ) -> Result<(), BuildError> {
        match step {
            BuildStep::Prebuilt(adapter) => Ok(prebuilt::build(
                adapter,
                params,
                reporter,
                self.wasm.as_ref(),
                self.files.as_ref(),
            )
            .await?),
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
