use crate::script::ScriptAdapterSyncError;
use async_trait::async_trait;
use camino::Utf8Path;
use snafu::Snafu;

#[async_trait]
pub trait Adapter {
    async fn sync(&self, canister_path: &Utf8Path) -> Result<(), AdapterSyncError>;
}

#[derive(Debug, Snafu)]
pub enum AdapterSyncError {
    #[snafu(transparent)]
    Script { source: ScriptAdapterSyncError },
}
