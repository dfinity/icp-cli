use async_trait::async_trait;

use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::manifest::canister::BuildStep;
use crate::prelude::*;

mod prebuilt;
mod script;

pub struct Params {
    pub path: PathBuf,
    pub output: PathBuf,
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
        stdio: Option<Sender<String>>,
    ) -> Result<(), BuildError>;
}

pub struct Builder;

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
            BuildStep::Script(adapter) => Ok(script::build(adapter, params, stdio).await?),
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
