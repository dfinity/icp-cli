use crate::context::{EnvGetAgentError, GetProjectError};
use crate::options::{IdentityOpt, NetworkOpt};
use crate::{context::Context, store_id::LookupError as LookupIdError};
use clap::Parser;
use ic_agent::AgentError;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterStopCmd {
    /// The name of the canister within the current project
    pub name: String,

    #[clap(flatten)]
    identity: IdentityOpt,

    #[clap(flatten)]
    network: NetworkOpt,
}

pub async fn exec(ctx: &Context, cmd: CanisterStopCmd) -> Result<(), CanisterStopError> {
    ctx.require_identity(cmd.identity.name());
    ctx.require_network(cmd.network.name());

    // Load the project manifest, which defines the canisters to be built.
    let pm = ctx.project()?;

    // Select canister to query
    let (_, c) = pm
        .canisters
        .iter()
        .find(|(_, c)| cmd.name == c.name)
        .ok_or(CanisterStopError::CanisterNotFound { name: cmd.name })?;

    // Lookup the canister id
    let cid = ctx.id_store.lookup(&c.name)?;

    let agent = ctx.agent()?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(agent);

    // Instruct management canister to stop canister
    mgmt.stop_canister(&cid).await?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterStopError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(transparent)]
    EnvGetAgent { source: EnvGetAgentError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    Agent { source: AgentError },
}
