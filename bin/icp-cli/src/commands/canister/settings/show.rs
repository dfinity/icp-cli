use clap::Parser;
use ic_agent::AgentError;
use snafu::Snafu;

use crate::context::Context;
use crate::options::{EnvironmentOpt, IdentityOpt};

#[derive(Debug, Parser)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

pub async fn exec(_ctx: &Context, _cmd: Cmd) -> Result<(), CommandError> {
    // TODO(vz): Implement.
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    Agent { source: AgentError },
}
