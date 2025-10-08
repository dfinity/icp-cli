use clap::Parser;
use ic_agent::AgentError;
use icp::{agent, identity, network};

use crate::{
    commands::{Context, ContextError},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Debug, Parser)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error(transparent)]
    EnvironmentNotFound(#[from] ContextError),

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error(transparent)]
    Lookup(#[from] LookupIdError),

    #[error(transparent)]
    Delete(#[from] AgentError),
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {

    // Load the environment
    let env = ctx.get_environment(cmd.environment.name()).await?;

    // Agent
    let agent = ctx.get_agent(&env, cmd.identity.into()).await?;

    // Ensure canister is included in the environment
    if !env.canisters.contains_key(&cmd.name) {
        return Err(CommandError::EnvironmentCanister {
            environment: env.name.to_owned(),
            canister: cmd.name.to_owned(),
        });
    }

    // Lookup the canister id
    let cid = ctx.ids.lookup(&Key {
        network: env.network.name.to_owned(),
        environment: env.name.to_owned(),
        canister: cmd.name.to_owned(),
    })?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Instruct management canister to delete canister
    mgmt.delete_canister(&cid).await?;

    // TODO(or.ricon): Remove the canister association with the network/environment

    Ok(())
}
