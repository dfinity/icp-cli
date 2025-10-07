use async_trait::async_trait;
use tokio::sync::mpsc::Sender;

use crate::canister::sync::{Step, Synchronize, SynchronizeError};

pub struct Assets;

#[async_trait]
impl Synchronize for Assets {
    async fn sync(
        &self,
        step: Step,
        stdio: Option<Sender<String>>,
    ) -> Result<(), SynchronizeError> {
        Ok(())
    }
}

// /// Configuration for a custom canister build adapter.
// #[derive(Clone, Debug, Deserialize, PartialEq, JsonSchema)]
// pub struct AssetsAdapter {
//     /// Directory used to synchronize an assets canister
//     #[serde(flatten)]
//     pub dir: DirField,
// }

// impl fmt::Display for AssetsAdapter {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         let dir = match &self.dir {
//             DirField::Dir(d) => format!("directory: {d}"),
//             DirField::Dirs(ds) => format!("{} directories", ds.len()),
//         };

//         write!(f, "({dir})")
//     }
// }

// #[async_trait]
// impl sync::Adapter for AssetsAdapter {
//     async fn sync(
//         &self,
//         canister_path: &Path,
//         canister_id: &Principal,
//         agent: &Agent,
//     ) -> Result<(), AdapterSyncError> {
//         // Normalize `dir` field based on whether it's a single dir or multiple.
//         let dirs = self.dir.as_vec();

//         #[allow(clippy::disallowed_types)]
//         let dirs = dirs
//             .iter()
//             // Paths are specified relative to the canister path
//             .map(|p| canister_path.join(p))
//             // Convert to PathBuf
//             .map(std::path::PathBuf::from)
//             .collect::<Vec<std::path::PathBuf>>();

//         #[allow(clippy::disallowed_types)]
//         let dirs: Vec<&std::path::Path> = dirs.iter().map(|p| p.as_path()).collect();

//         // ic-asset requires a logger, so provide it a nop logger
//         let logger = slog::Logger::root(slog::Discard, slog::o!());

//         // Prepare canister client
//         let canister = Canister::builder()
//             .with_canister_id(canister_id.to_owned())
//             .with_agent(agent)
//             .build()
//             .map_err(|err| AssetsAdapterSyncError::CanisterBuilder { source: err })?;

//         // Synchronize assets to canister
//         ic_asset::sync(
//             &canister, // canister
//             &dirs,     // dirs
//             false,     // no_delete
//             &logger,   // logger
//             None,      // progress
//         )
//         .await
//         .map_err(|err| AssetsAdapterSyncError::Sync { source: err })?;

//         Ok(())
//     }
// }

// #[derive(Debug, Snafu)]
// #[snafu(context(suffix(SyncSnafu)))]
// pub enum AssetsAdapterSyncError {
//     #[snafu(transparent)]
//     CanisterBuilder { source: CanisterBuilderError },

//     #[snafu(transparent)]
//     Sync { source: SyncError },
// }
