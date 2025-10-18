use clap::Args;
use icp::identity;

use crate::commands::canister::snapshot::SnapshotId;
use crate::commands::{Context, Mode};
use crate::options::{EnvironmentOpt, IdentityOpt};

#[derive(Debug, Args)]
pub struct CreateArgs {
    /// The name of the canister within the current project
    name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,

    /// If a snapshot ID is specified, this snapshot will replace it and reuse the ID.
    replace: Option<SnapshotId>,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),
}

pub async fn exec(ctx: &Context, _args: &CreateArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(_) => {
            unimplemented!("project mode is not implemented yet");
        }
    }
}
