use crate::{
    commands::args::CanisterCommandArgs, operations::settings::SyncSettingsOperationError,
};

use clap::Args;
use ic_utils::interfaces::ManagementCanister;
use icp::{
    LoadError,
    context::{
        AssertEnvContainsCanisterError, CanisterSelection, Context, GetCanisterIdAndAgentError,
        GetEnvironmentError,
    },
};
use snafu::Snafu;

#[derive(Debug, Args)]
pub(crate) struct SyncArgs {
    #[command(flatten)]
    cmd_args: CanisterCommandArgs,
}

#[derive(Debug, Snafu)]
pub(crate) enum CommandError {
    #[snafu(transparent)]
    GetIdAndAgent { source: GetCanisterIdAndAgentError },

    #[snafu(transparent)]
    GetEnvironment { source: GetEnvironmentError },

    #[snafu(display("Canister name must be used for settings sync"))]
    PrincipalCanister,

    #[snafu(transparent)]
    LoadProject { source: LoadError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(transparent)]
    EnvironmentCanisterNotFound {
        source: AssertEnvContainsCanisterError,
    },

    #[snafu(transparent)]
    SyncSettingsError { source: SyncSettingsOperationError },
}

pub(crate) async fn exec(ctx: &Context, args: &SyncArgs) -> Result<(), CommandError> {
    let selections = args.cmd_args.selections();
    let CanisterSelection::Named(name) = &selections.canister else {
        return PrincipalCanisterSnafu.fail();
    };

    let p = ctx.project.load().await?;

    let Some((_, canister)) = p.canisters.get(name) else {
        return CanisterNotFoundSnafu { name }.fail();
    };
    ctx.assert_env_contains_canister(name, &selections.environment)
        .await?;

    let (cid, agent) = ctx
        .get_canister_id_and_agent(
            &selections.canister,
            &selections.environment,
            &selections.network,
            &selections.identity,
        )
        .await?;

    let mgmt = ManagementCanister::create(&agent);

    crate::operations::settings::sync_settings(&mgmt, &cid, canister).await?;
    Ok(())
}
