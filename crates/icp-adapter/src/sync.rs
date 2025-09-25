use crate::{assets::AssetsAdapterSyncError, script::ScriptAdapterSyncError};
use async_trait::async_trait;
use camino::Utf8Path as Path;
use ic_agent::{Agent, export::Principal};
use snafu::Snafu;

#[async_trait]
pub trait Adapter {
    async fn sync(
        &self,
        canister_path: &Path,
        canister_id: &Principal,
        agent: &Agent,
    ) -> Result<(), AdapterSyncError>;
}

#[derive(Debug, Snafu)]
pub enum AdapterSyncError {
    #[snafu(transparent)]
    Script { source: ScriptAdapterSyncError },

    #[snafu(transparent)]
    Assets { source: AssetsAdapterSyncError },
}
