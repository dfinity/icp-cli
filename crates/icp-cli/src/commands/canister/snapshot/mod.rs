use std::str::FromStr;

use clap::Subcommand;

pub(crate) mod create;
pub(crate) mod delete;
pub(crate) mod download;
pub(crate) mod list;
pub(crate) mod restore;
pub(crate) mod upload;

/// Commands to manage canister snapshots
#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    Create(create::CreateArgs),
    Delete(delete::DeleteArgs),
    Download(download::DownloadArgs),
    List(list::ListArgs),
    Restore(restore::RestoreArgs),
    Upload(upload::UploadArgs),
}

/// A hex-encoded snapshot ID.
#[derive(Debug, Clone)]
pub(crate) struct SnapshotId(pub Vec<u8>);

impl FromStr for SnapshotId {
    type Err = hex::FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        hex::decode(s).map(SnapshotId)
    }
}
