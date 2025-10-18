use clap::Args;
use icp::identity;

use crate::commands::canister::snapshot::SnapshotId;
use crate::commands::{Context, Mode};
use crate::options::{EnvironmentOpt, IdentityOpt};

#[derive(Debug, Args)]
pub struct LoadArgs {
    /// The name of the canister within the current project
    name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,

    /// The ID of the snapshot to load.
    snapshot: SnapshotId,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),
}

pub async fn exec(ctx: &Context, _args: &LoadArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(_) => {
            unimplemented!("project mode is not implemented yet");
        }
    }
}
