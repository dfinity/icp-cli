use crate::{
    commands::args::CanisterCommandArgs, operations::settings::SyncSettingsOperationError,
};

use clap::Args;
use ic_utils::interfaces::ManagementCanister;
use icp::context::{
    CanisterSelection, Context, GetCanisterIdAndAgentError, GetEnvCanisterError,
    GetEnvironmentError,
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
    GetEnvCanister { source: GetEnvCanisterError },

    #[snafu(transparent)]
    SyncSettingsError { source: SyncSettingsOperationError },
}

pub(crate) async fn exec(ctx: &Context, args: &SyncArgs) -> Result<(), CommandError> {
    let selections = args.cmd_args.selections();
    let CanisterSelection::Named(name) = &selections.canister else {
        return PrincipalCanisterSnafu.fail();
    };

    let (_, canister) = ctx
        .get_canister_and_path_for_env(name, &selections.environment)
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

    crate::operations::settings::sync_settings(&mgmt, &cid, &canister).await?;
    Ok(())
}
