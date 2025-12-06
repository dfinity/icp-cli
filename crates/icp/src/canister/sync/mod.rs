use async_trait::async_trait;
use candid::Principal;
use ic_agent::Agent;
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::manifest::canister::SyncStep;
use crate::prelude::*;

mod assets;
mod script;

pub struct Params {
    pub path: PathBuf,
    pub cid: Principal,
}

#[derive(Debug, Snafu)]
pub enum SynchronizeError {
    #[snafu(transparent)]
    Script { source: super::script::ScriptError },

    #[snafu(transparent)]
    Assets { source: assets::AssetsError },
}

#[async_trait]
pub trait Synchronize: Sync + Send {
    async fn sync(
        &self,
        step: &SyncStep,
        params: &Params,
        agent: &Agent,
        stdio: Option<Sender<String>>,
    ) -> Result<(), SynchronizeError>;
}

pub struct Syncer;

#[async_trait]
impl Synchronize for Syncer {
    async fn sync(
        &self,
        step: &SyncStep,
        params: &Params,
        agent: &Agent,
        stdio: Option<Sender<String>>,
    ) -> Result<(), SynchronizeError> {
        match step {
            SyncStep::Assets(adapter) => Ok(assets::sync(adapter, params, agent).await?),
            SyncStep::Script(adapter) => Ok(script::sync(adapter, params, stdio).await?),
        }
    }
}

#[cfg(test)]
/// Unimplemented mock implementation of `Synchronize`.
/// All methods panic with `unimplemented!()` when called.
pub struct UnimplementedMockSyncer;

#[cfg(test)]
#[async_trait]
impl Synchronize for UnimplementedMockSyncer {
    async fn sync(
        &self,
        _step: &SyncStep,
        _params: &Params,
        _agent: &Agent,
        _stdio: Option<Sender<String>>,
    ) -> Result<(), SynchronizeError> {
        unimplemented!("UnimplementedMockSyncer::sync")
    }
}
