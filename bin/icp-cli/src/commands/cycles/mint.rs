use bigdecimal::BigDecimal;
use clap::Parser;
use snafu::Snafu;

use crate::{
    context::{Context, ContextGetAgentError, GetProjectError},
    options::{EnvironmentOpt, IdentityOpt},
};

/// Memo value for mint operations.
/// This constant represents the ASCII encoding of the string "MINT" as a 64-bit unsigned integer.
const MEMO: u64 = 0x544e494d;

#[derive(Debug, Parser)]
pub struct Cmd {
    /// ICP amount to convert to cycles (conflicts with cycles option)
    #[clap(conflicts_with = "cycles")]
    pub icp: Option<BigDecimal>,

    /// Cycles amount to mint (conflicts with icp option)
    #[clap(conflicts_with = "icp")]
    pub cycles: Option<u128>,

    #[command(flatten)]
    pub identity: IdentityOpt,

    #[command(flatten)]
    pub environment: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest
    let pm = ctx.project()?;

    // Load identity
    ctx.require_identity(cmd.identity.name());

    // Load target environment
    let env = pm
        .environments
        .iter()
        .find(|&v| v.name == cmd.environment.name())
        .ok_or(CommandError::EnvironmentNotFound {
            name: cmd.environment.name().to_owned(),
        })?;

    // TODO(or.ricon): Support default networks (`local` and `ic`)
    //
    let network = env
        .network
        .as_ref()
        .expect("no network specified in environment");

    // Setup network
    ctx.require_network(network);

    // Prepare agent
    let agent = ctx.agent()?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(display("project does not contain an environment named '{name}'"))]
    EnvironmentNotFound { name: String },

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },
}
