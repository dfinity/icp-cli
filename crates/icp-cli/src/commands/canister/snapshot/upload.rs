use clap::Args;
use icp::prelude::PathBuf;

use crate::commands::args;
use crate::commands::canister::snapshot::{SnapshotId, directory_parser};
use icp::context::Context;

#[derive(Debug, Args)]
pub struct UploadArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,

    /// If a snapshot ID is specified, this snapshot will replace it and reuse the ID.
    #[arg(long)]
    replace: Option<SnapshotId>,

    /// The directory to upload the snapshot from.
    #[arg(long, value_parser = directory_parser)]
    dir: PathBuf,

    /// The snapshot ID to resume uploading to.
    #[arg(short, long)]
    resume: Option<SnapshotId>,

    /// The number of concurrent uploads to perform.
    #[arg(long, default_value = "3")]
    concurrency: usize,
}

pub async fn exec(_ctx: &Context, _args: &UploadArgs) -> Result<(), anyhow::Error> {
    unimplemented!("canister snapshot upload is not yet implemented");
}
