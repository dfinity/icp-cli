use clap::Args;
use ic_agent::AgentError;
use icp::{agent, identity, network};

use crate::{
    commands::{Context, Mode},
    options::{EnvironmentOpt, IdentityOpt},
    store_id::{Key, LookupError as LookupIdError},
};

#[derive(Debug, Args)]
pub(crate) struct DeleteArgs {
    /// The name of the canister within the current project
    pub(crate) name: String,

    #[command(flatten)]
    identity: IdentityOpt,

    #[command(flatten)]
    environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("project does not contain an environment named '{name}'")]
    EnvironmentNotFound { name: String },

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error("environment '{environment}' does not include canister '{canister}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error("{err}")]
    Todo { err: String },

    #[error(transparent)]
    Lookup(#[from] LookupIdError),

    #[error(transparent)]
    Delete(#[from] AgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &DeleteArgs) -> Result<(), CommandError> {
    ctx.with_environment(args.environment.clone());
    ctx.with_identity(args.identity.clone());

    let agent = ctx
        .get_agent()
        .await
        .map_err(|e| CommandError::Todo { err: e.to_string() })?;

    let canister = ctx
        .get_canister_principal(&args.name)
        .await
        .map_err(|e| CommandError::Todo { err: e.to_string() })?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Instruct management canister to delete canister
    mgmt.delete_canister(&canister).await?;

    // TODO(or.ricon): Remove the canister association with the network/environment

    Ok(())
}
