use clap::Args;
use icp::{identity, prelude::PathBuf};

use crate::commands::{
    args,
    canister::snapshot::{SnapshotId, directory_parser},
};
use icp::context::Context;

#[derive(Debug, Args)]
pub struct DownloadArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

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

pub async fn exec(_ctx: &Context, _args: &DownloadArgs) -> Result<(), CommandError> {
    unimplemented!("project mode is not implemented yet");
}
