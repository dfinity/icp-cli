use clap::Args;
use icp::identity;
use std::path::PathBuf;

use crate::commands::canister::snapshot::{SnapshotId, directory_parser};
use crate::commands::{Context, Mode};
use crate::options::{EnvironmentOpt, IdentityOpt};

#[derive(Debug, Args)]
pub struct DownloadArgs {
    /// The name of the canister within the current project
    name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,

    /// The ID of the snapshot to download.
    snapshot: SnapshotId,

    /// The directory to download the snapshot to.
    #[arg(long, value_parser = directory_parser)]
    dir: PathBuf,

    /// Whether to resume the download if the previous snapshot download failed.
    #[arg(short, long, default_value = "false")]
    resume: bool,

    /// The number of concurrent downloads to perform.
    #[arg(long, default_value = "3")]
    concurrency: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),
}

pub async fn exec(ctx: &Context, _args: &DownloadArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(_) => {
            unimplemented!("project mode is not implemented yet");
        }
    }
}
