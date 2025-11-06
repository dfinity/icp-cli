use crate::{
    operations::settings::SyncSettingsOperationError,
    options::{EnvironmentOpt, IdentityOpt, NetworkOpt},
};

use clap::Args;
use ic_utils::interfaces::ManagementCanister;
use icp::{
    LoadError,
    context::{
        Context, EnvironmentSelection, GetAgentForEnvError, GetCanisterIdForEnvError,
        GetEnvironmentError,
    },
};
use snafu::Snafu;

#[derive(Debug, Args)]
pub(crate) struct SyncArgs {
    name: String,
    #[command(flatten)]
    pub(crate) network: NetworkOpt,
    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
    #[command(flatten)]
    pub(crate) identity: IdentityOpt,
}

#[derive(Debug, Snafu)]
pub(crate) enum CommandError {
    #[snafu(transparent)]
    GetAgentForEnv { source: GetAgentForEnvError },
    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },
    #[snafu(transparent)]
    GetCanisterIdForEnv { source: GetCanisterIdForEnvError },
    #[snafu(transparent)]
    LoadProject { source: LoadError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("environment '{environment}' does not include canister '{name}'"))]
    EnvironmentCanisterNotFound { name: String, environment: String },

    #[snafu(transparent)]
    SyncSettingsError { source: SyncSettingsOperationError },
}

pub(crate) async fn exec(ctx: &Context, args: &SyncArgs) -> Result<(), CommandError> {
    let environment_selection: EnvironmentSelection = args.environment.clone().into();
    let name = &args.name;

    let p = ctx.project.load().await?;
    let env = ctx.get_environment(&environment_selection).await?;

    let Some((_, canister)) = p.canisters.get(name) else {
        return CanisterNotFoundSnafu { name }.fail();
    };

    if !env.canisters.contains_key(&args.name) {
        return EnvironmentCanisterNotFoundSnafu {
            environment: &env.name,
            name,
        }
        .fail();
    }

    let agent = ctx
        .get_agent_for_env(&args.identity.clone().into(), &environment_selection)
        .await?;
    let cid = ctx
        .get_canister_id_for_env(&args.name, &environment_selection)
        .await?;
    let mgmt = ManagementCanister::create(&agent);

    crate::operations::settings::sync_settings(&mgmt, &cid, canister).await?;
    Ok(())
}
