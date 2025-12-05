use std::sync::Arc;

use async_trait::async_trait;

use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::{canister::script::ScriptError, manifest::canister::BuildStep, prelude::*};

mod prebuilt;

pub struct Params {
    pub path: PathBuf,
    pub output: PathBuf,
}

#[derive(Debug, Snafu)]
pub enum BuildError {
    #[snafu(transparent)]
    Script { source: ScriptError },
    #[snafu(transparent)]
    Prebuilt { source: prebuilt::PrebuiltError },
}

#[async_trait]
pub trait Build: Sync + Send {
    async fn build(
        &self,
        step: &BuildStep,
        params: &Params,
        stdio: Option<Sender<String>>,
    ) -> Result<(), BuildError>;
}

pub struct Builder {
    pub script: Arc<dyn Build>,
}

#[async_trait]
impl Build for Builder {
    async fn build(
        &self,
        step: &BuildStep,
        params: &Params,
        stdio: Option<Sender<String>>,
    ) -> Result<(), BuildError> {
        match step {
            BuildStep::Prebuilt(adapter) => Ok(prebuilt::build(adapter, params, stdio).await?),
            BuildStep::Script(_) => self.script.build(step, params, stdio).await,
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
        _step: &BuildStep,
        _params: &Params,
        _stdio: Option<Sender<String>>,
    ) -> Result<(), BuildError> {
        unimplemented!("UnimplementedMockBuilder::build")
    }
}
