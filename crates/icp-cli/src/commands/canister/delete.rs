use clap::Args;
use ic_agent::AgentError;
use icp::{agent, identity, network};

use crate::{
    commands::{
        Context, ContextError,
        args::{ArgContext, ArgumentError},
    },
    options::{EnvironmentOpt, IdentityOpt},
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

    #[error(transparent)]
    Access(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error(transparent)]
    Argument(#[from] ArgumentError),

    #[error(transparent)]
    Context(#[from] ContextError),

    #[error(transparent)]
    Delete(#[from] AgentError),
}

pub(crate) async fn exec(ctx: &Context, args: &DeleteArgs) -> Result<(), CommandError> {
    let arg_ctx = ArgContext::new(
        ctx,
        args.environment.clone(),
        None,
        args.identity.clone(),
        vec![&args.name],
    )
    .await?;

    let agent = ctx.get_agent(&arg_ctx).await?;
    let canister_id = ctx.resolve_canister_id(&arg_ctx, &args.name)?;

    // Management Interface
    let mgmt = ic_utils::interfaces::ManagementCanister::create(&agent);

    // Instruct management canister to delete canister
    mgmt.delete_canister(&canister_id).await?;

    // TODO(or.ricon): Remove the canister association with the network/environment

    Ok(())
}
