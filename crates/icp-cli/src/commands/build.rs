use clap::Args;
use futures::future::try_join_all;
use icp::context::{Context, EnvironmentSelection, GetEnvironmentError};

use crate::{
    operations::build::{BuildOperationError, build_many_with_progress_bar},
    options::EnvironmentOpt,
};

#[derive(Debug, Args)]
pub(crate) struct BuildArgs {
    /// Canister names (if empty, build all canisters in environment)
    pub(crate) canisters: Vec<String>,

    #[command(flatten)]
    pub(crate) environment: EnvironmentOpt,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    BuildOperation(#[from] BuildOperationError),

    #[error(transparent)]
    GetEnvironment(#[from] GetEnvironmentError),

    #[error(transparent)]
    GetEnvCanister(#[from] icp::context::GetEnvCanisterError),

    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub(crate) async fn exec(ctx: &Context, args: &BuildArgs) -> Result<(), CommandError> {
    // Get environment selection
    let environment_selection: EnvironmentSelection = args.environment.clone().into();

    // Load target environment
    let env = ctx.get_environment(&environment_selection).await?;

    // Determine which canisters to build
    let cnames = match args.canisters.is_empty() {
        // No canisters specified - build all in environment
        true => env.canisters.keys().cloned().collect(),

        // Individual canisters specified
        false => args.canisters.clone(),
    };

    // Skip doing any work if no canisters are targeted
    if cnames.is_empty() {
        return Ok(());
    }

    let canisters_to_build = try_join_all(
        cnames
            .iter()
            .map(|name| ctx.get_canister_and_path_for_env(name, &environment_selection)),
    )
    .await?;
    // Build the selected canisters
    let _ = ctx.term.write_line("Building canisters:");

    build_many_with_progress_bar(
        canisters_to_build,
        ctx.builder.clone(),
        ctx.artifacts.clone(),
        &ctx.term,
        ctx.debug,
    )
    .await?;

    let _ = ctx.term.write_line("\nCanisters built successfully");

    Ok(())
}
