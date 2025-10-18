use clap::Args;
use icp::{identity, prelude::PathBuf};

use crate::commands::canister::snapshot::{SnapshotId, directory_parser};
use crate::commands::{Context, Mode};
use crate::options::{EnvironmentOpt, IdentityOpt};

#[derive(Debug, Args)]
pub struct UploadArgs {
    /// The name of the canister within the current project
    name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,

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

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),
}

pub async fn exec(ctx: &Context, _args: &UploadArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(_) => {
            unimplemented!("project mode is not implemented yet");
        }
    }
}
