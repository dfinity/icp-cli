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

/// Where a build step left the module.
///
/// The steps of a build hand the module on through one file, [`Params::output`]:
/// a step may write it there, or change what an earlier step wrote. A step
/// that already holds the whole module, and has nothing to do to it but pass
/// it on, returns it instead. Writing it to `output` first would put a second
/// copy on disk only for the build to read it straight back, and where the
/// disk is memory that copy is as large as the module.
#[derive(Debug)]
pub enum Module {
    /// At [`Params::output`], if the build has one yet: this step wrote it
    /// there, changed it there, or left alone what the steps before it made.
    AtOutput,
    /// Here, and not at [`Params::output`]. It replaces whatever the steps
    /// before it made, and whoever runs the steps writes it to `output` if
    /// another step follows to look for it.
    Bytes(Vec<u8>),
}

#[derive(Debug, Snafu)]
pub enum BuildError {
    #[cfg(feature = "host")]
    #[snafu(transparent)]
    Script { source: super::script::ScriptError },
    #[cfg(feature = "host")]
    #[snafu(transparent)]
    Prebuilt { source: prebuilt::PrebuiltError },
    /// A [`Build`] this crate cannot name failed. How it runs a step is its own
    /// business, so the cause is carried whole and displayed as itself.
    ///
    /// This is the only variant a build off this machine has, and without it
    /// the enum would be uninhabited there.
    #[snafu(transparent)]
    Other {
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

impl BuildError {
    /// Wraps an implementation's own error for the trait boundary.
    pub fn new(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Other {
            source: Box::new(source),
        }
    }
}

#[async_trait]
pub trait Build: Sync + Send {
    async fn build(
        &self,
        step: &BuildStep,
        params: &Params,
        reporter: &StepReporter,
    ) -> Result<Module, BuildError>;
}

/// Runs each build step where it has to be run: a script step in a subprocess,
/// a pre-built step by asking [`wasm::Fetch`] for the module and reading it
/// through `files`.
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
    ) -> Result<Module, BuildError> {
        match step {
            BuildStep::Prebuilt(adapter) => Ok(Module::Bytes(
                prebuilt::build(
                    adapter,
                    params,
                    reporter,
                    self.wasm.as_ref(),
                    self.files.as_ref(),
                )
                .await?,
            )),
            BuildStep::Script(adapter) => {
                script::build(adapter, params, reporter).await?;
                Ok(Module::AtOutput)
            }
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
    ) -> Result<Module, BuildError> {
        unimplemented!("UnimplementedMockBuilder::build")
    }
}
