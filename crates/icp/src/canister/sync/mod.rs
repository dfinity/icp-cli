use std::sync::Arc;

use async_trait::async_trait;
use candid::Principal;
use ic_agent::Agent;
use snafu::prelude::*;
use tokio::sync::mpsc::Sender;

use crate::canister::{assets::AssetsError, script::ScriptError};
use crate::manifest::canister::SyncStep;
use crate::prelude::*;

pub struct Params {
    pub path: PathBuf,
    pub cid: Principal,
}

#[derive(Debug, Snafu)]
pub enum SynchronizeError {
    #[snafu(transparent)]
    Script { source: ScriptError },

    #[snafu(transparent)]
    Assets { source: AssetsError },
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

pub struct Syncer {
    pub assets: Arc<dyn Synchronize>,
    pub script: Arc<dyn Synchronize>,
}

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
            SyncStep::Assets(_) => self.assets.sync(step, params, agent, stdio).await,
            SyncStep::Script(_) => self.script.sync(step, params, agent, stdio).await,
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
