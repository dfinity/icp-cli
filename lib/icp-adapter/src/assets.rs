use crate::sync::{self, AdapterSyncError};
use async_trait::async_trait;
use camino::Utf8Path;
use ic_agent::{Agent, export::Principal};
use ic_asset::error::SyncError;
use ic_utils::{Canister, canister::CanisterBuilderError};
use serde::Deserialize;
use snafu::Snafu;
use tokio::task::JoinError;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DirField {
    /// Directory used to synchronize an assets canister
    Dir(String),

    /// Set of directories used to synchronize an assets canister
    Dirs(Vec<String>),
}

impl DirField {
    fn as_vec(&self) -> Vec<String> {
        match self {
            Self::Dir(dir) => vec![dir.clone()],
            Self::Dirs(dirs) => dirs.clone(),
        }
    }
}

/// Configuration for a custom canister build adapter.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct AssetsAdapter {
    /// Directory used to synchronize an assets canister
    #[serde(flatten)]
    pub dir: DirField,
}

#[async_trait]
impl sync::Adapter for AssetsAdapter {
    async fn sync(
        &self,
        _: &Utf8Path,
        canister_id: &Principal,
        agent: &Agent,
    ) -> Result<(), AdapterSyncError> {
        // Normalize `dir` field based on whether it's a single dir or multiple.
        let dirs = self.dir.as_vec();

        #[allow(clippy::disallowed_types)]
        let dirs = dirs
            .iter()
            .map(std::path::PathBuf::from)
            .collect::<Vec<std::path::PathBuf>>();

        // Clone the agent and canister_id to move into the async task
        let agent = agent.clone();
        let canister_id = canister_id.to_owned();

        // ic-asset requires a logger, so provide it a nop logger
        let logger = slog::Logger::root(slog::Discard, slog::o!());

        // Spawn a local task to perform synchornization
        // This is required because AssetSyncProgressRenderer is !Send
        tokio::task::spawn_local(async move {
            // Convert PathBuf to &Path inside the async block
            #[allow(clippy::disallowed_types)]
            let dirs: Vec<&std::path::Path> = dirs.iter().map(|p| p.as_path()).collect();

            // Prepare canister client inside the async block
            let canister = Canister::builder()
                .with_canister_id(canister_id)
                .with_agent(&agent)
                .build()
                .map_err(|err| AssetsAdapterSyncError::CanisterBuilder { source: err })?;

            // Synchronize assets to canister
            ic_asset::sync(
                &canister, // canister
                &dirs,     // dirs
                false,     // no_delete
                &logger,   // logger
                None,      // progress
            )
            .await
            .map_err(|err| AssetsAdapterSyncError::Sync { source: err })?;

            Ok::<_, AssetsAdapterSyncError>(())
        })
        .await
        .map_err(|err| AssetsAdapterSyncError::Join { source: err })??;

        Ok(())
    }
}

#[derive(Debug, Snafu)]
#[snafu(context(suffix(SyncSnafu)))]
pub enum AssetsAdapterSyncError {
    #[snafu(transparent)]
    CanisterBuilder { source: CanisterBuilderError },

    #[snafu(transparent)]
    Sync { source: SyncError },

    #[snafu(transparent)]
    Join { source: JoinError },
}
