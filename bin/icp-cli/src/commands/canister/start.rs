use crate::context::{ContextGetAgentError, GetProjectError};
use crate::options::{EnvironmentOpt, IdentityOpt};
use crate::{context::Context, store_id::LookupError as LookupIdError};
use clap::Parser;
use ic_agent::AgentError;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterStartCmd {
    /// The name of the canister within the current project
    pub name: String,

    #[clap(flatten)]
    identity: IdentityOpt,

    #[clap(flatten)]
    network: EnvironmentOpt,
}

pub async fn exec(ctx: &Context, cmd: CanisterStartCmd) -> Result<(), CanisterStartError> {
    ctx.require_identity(cmd.identity.name());
    ctx.require_network(cmd.network.name());

    // Load the project manifest, which defines the canisters to be built.
    let pm = ctx.project()?;

    // Select canister to query
    let (_, c) = pm
        .canisters
        .iter()
        .find(|(_, c)| cmd.name == c.name)
        .ok_or(CanisterStartError::CanisterNotFound { name: cmd.name })?;

    // Lookup the canister id
    let cid = ctx.id_store.lookup(&c.name)?;

    let agent = ctx.agent()?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(agent);

    // Instruct management canister to start canister
    mgmt.start_canister(&cid).await?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CanisterStartError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(transparent)]
    GetAgent { source: ContextGetAgentError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    Agent { source: AgentError },
}
