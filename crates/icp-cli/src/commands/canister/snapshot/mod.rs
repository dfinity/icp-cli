use clap::Subcommand;
use ic_management_canister_types::{CanisterStatusType, UploadCanisterSnapshotMetadataResult};
use icp::prelude::PathBuf;
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

pub(crate) mod create;
pub(crate) mod delete;
pub(crate) mod download;
pub(crate) mod list;
pub(crate) mod load;
pub(crate) mod upload;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotId(Vec<u8>);

impl Display for SnapshotId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(&self.0))
    }
}

impl FromStr for SnapshotId {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(hex::decode(s)?))
    }
}

impl From<UploadCanisterSnapshotMetadataResult> for SnapshotId {
    fn from(canister_snapshot_id: UploadCanisterSnapshotMetadataResult) -> Self {
        SnapshotId(canister_snapshot_id.snapshot_id)
    }
}

fn ensure_canister_stopped(status: CanisterStatusType, canister: &str) -> Result<(), CommandError> {
    match status {
        CanisterStatusType::Stopped => Ok(()),
        CanisterStatusType::Running => Err(CommandError::CanisterNotStopped(format!(
            "Canister {canister} is running. Run 'icp canister stop' to stop it first"
        ))),
        CanisterStatusType::Stopping => Err(CommandError::CanisterNotStopped(format!(
            "Canister {canister} is stopping but is not yet stopped. Wait a few seconds and try again"
        ))),
    }
}

fn directory_parser(path: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(path);
    if path.is_dir() {
        Ok(path)
    } else {
        Err(format!(
            "Path '{}' does not exist or is not a directory.",
            path
        ))
    }
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
    Create(create::CreateArgs),
    Delete(delete::DeleteArgs),
    Download(download::DownloadArgs),
    List(list::ListArgs),
    Load(load::LoadArgs),
    Upload(upload::UploadArgs),
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("{0}")]
    CanisterNotStopped(String),
}
