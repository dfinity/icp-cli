// This is a temporary placeholder command
// For now it's only used to set environment variables
// Eventually we will add support for canister settings operation

use anyhow::anyhow;
use clap::Args;
use ic_utils::interfaces::ManagementCanister;
use icp::{
    agent,
    context::{CanisterSelection, GetAgentForEnvError, GetEnvironmentError},
    identity, network,
    store_id::LookupIdError,
};

use icp::context::Context;

use crate::{
    commands::args,
    operations::binding_env_vars::{BindingEnvVarsOperationError, set_env_vars_for_canister},
};

#[derive(Debug, Args)]
pub(crate) struct BindingArgs {
    #[command(flatten)]
    pub(crate) cmd_args: args::CanisterCommandArgs,
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
    Agent(#[from] agent::CreateAgentError),

    #[error(transparent)]
    LookupId(#[from] LookupIdError),

    #[error(transparent)]
    GetAgentForEnv(#[from] GetAgentForEnvError),

    #[error(transparent)]
    GetEnvironment(#[from] GetEnvironmentError),

    #[error(transparent)]
    BindingEnvVars(#[from] BindingEnvVarsOperationError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[allow(unused)]
pub(crate) async fn exec(ctx: &Context, args: &BindingArgs) -> Result<(), CommandError> {
    let selections = args.cmd_args.selections();
    let canister = match selections.canister {
        CanisterSelection::Named(name) => name,
        CanisterSelection::Principal(_) => {
            Err(anyhow!("Cannot set environment variables by principal"))?
        }
    };

    // Load target environment
    let env = ctx.get_environment(&selections.environment).await?;
    let (_, canister_info) = env.get_canister_info(&canister).map_err(|e| anyhow!(e))?;

    // Get canister ID
    let canister_id = ctx
        .get_canister_id_for_env(&canister, &selections.environment)
        .await
        .map_err(|e| anyhow!(e))?;

    // Agent
    let agent = ctx
        .get_agent_for_env(&selections.identity, &selections.environment)
        .await?;

    // Get the list of name to canister id for this environment
    // We need this to inject the `PUBLIC_CANISTER_ID:` environment variables
    let canister_list = ctx.ids.lookup_by_environment(&env.name)?;
    let binding_vars = canister_list
        .iter()
        .map(|(n, p)| (format!("PUBLIC_CANISTER_ID:{n}"), p.to_text()))
        .collect::<Vec<(_, _)>>();

    // Management Interface
    let mgmt = ManagementCanister::create(&agent);

    // Set environment variables for the single canister
    set_env_vars_for_canister(&mgmt, &canister_id, &canister_info, &binding_vars).await?;

    let _ = ctx.term.write_line(&format!(
        "Environment variables updated successfully for canister {canister}"
    ));

    Ok(())
}
