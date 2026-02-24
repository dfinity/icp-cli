use async_trait::async_trait;
use candid::Principal;
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::manifest::canister::PreinstallStep;
use crate::prelude::*;

mod script;

pub struct Params {
    pub path: PathBuf,
    pub wasm_path: PathBuf,
    pub cid: Principal,
    pub environment: String,
}

#[derive(Debug, Snafu)]
pub enum PreinstallError {
    #[snafu(transparent)]
    Script { source: super::script::ScriptError },
}

#[async_trait]
pub trait Preinstall: Sync + Send {
    async fn preinstall(
        &self,
        step: &PreinstallStep,
        params: &Params,
        stdio: Option<Sender<String>>,
    ) -> Result<(), PreinstallError>;
}

pub struct Preinstaller;

#[async_trait]
impl Preinstall for Preinstaller {
    async fn preinstall(
        &self,
        step: &PreinstallStep,
        params: &Params,
        stdio: Option<Sender<String>>,
    ) -> Result<(), PreinstallError> {
        match step {
            PreinstallStep::Script(adapter) => {
                Ok(script::preinstall(adapter, params, stdio).await?)
            }
        }
    }
}

#[cfg(test)]
pub struct UnimplementedMockPreinstaller;

#[cfg(test)]
#[async_trait]
impl Preinstall for UnimplementedMockPreinstaller {
    async fn preinstall(
        &self,
        _step: &PreinstallStep,
        _params: &Params,
        _stdio: Option<Sender<String>>,
    ) -> Result<(), PreinstallError> {
        unimplemented!("mock preinstaller called")
    }
}
