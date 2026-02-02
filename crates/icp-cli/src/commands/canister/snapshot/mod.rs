use std::str::FromStr;

use clap::Subcommand;

pub(crate) mod create;
pub(crate) mod delete;
pub(crate) mod list;
pub(crate) mod restore;

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Create a snapshot of a canister's state
    Create(create::CreateArgs),
    /// Delete a canister snapshot
    Delete(delete::DeleteArgs),
    /// List all snapshots for a canister
    List(list::ListArgs),
    /// Restore a canister from a snapshot
    Restore(restore::RestoreArgs),
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
