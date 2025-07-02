use crate::env::{EnvGetAgentError, GetProjectError};
use crate::options::NetworkOpt;
use crate::{env::Env, store_id::LookupError as LookupIdError};
use clap::Parser;
use ic_agent::AgentError;
use snafu::Snafu;

#[derive(Debug, Parser)]
pub struct CanisterStartCmd {
    /// The name of the canister within the current project
    pub name: String,

    #[clap(flatten)]
    network: NetworkOpt,
}

pub async fn exec(env: &Env, cmd: CanisterStartCmd) -> Result<(), CanisterStartError> {
    env.require_network(cmd.network.name());

    // Load the project manifest, which defines the canisters to be built.
    let pm = env.project()?;

    // Select canister to query
    let (_, c) = pm
        .canisters
        .iter()
        .find(|(_, c)| cmd.name == c.name)
        .ok_or(CanisterStartError::CanisterNotFound { name: cmd.name })?;

    // Lookup the canister id
    let cid = env.id_store.lookup(&c.name)?;

    let agent = env.agent()?;

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
    EnvGetAgent { source: EnvGetAgentError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(transparent)]
    LookupCanisterId { source: LookupIdError },

    #[snafu(transparent)]
    Agent { source: AgentError },
}
