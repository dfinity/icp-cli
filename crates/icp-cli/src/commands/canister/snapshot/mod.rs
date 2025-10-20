use clap::Subcommand;
use ic_agent::AgentError;
use ic_management_canister_types::{CanisterStatusType, UploadCanisterSnapshotMetadataResult};
use icp::{agent, identity, network, prelude::PathBuf};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use crate::store_id::LookupError as LookupIdError;

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
            "Canister {canister} is running. Run `dfx canister stop` to stop it first"
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
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error(transparent)]
    Lookup(#[from] LookupIdError),

    #[error(transparent)]
    Status(#[from] AgentError),

    #[error("{0}")]
    CanisterNotStopped(String),
}
