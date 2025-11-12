use anyhow::Context as _;
use clap::Args;
use icp::context::Context;

use crate::operations::build::{BuildOperationError, build_many_with_progress_bar};

#[derive(Args, Debug)]
pub(crate) struct BuildArgs {
    /// The name of the canister within the current project
    pub(crate) name: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error("project does not contain a canister named '{name}'")]
    CanisterNotFound { name: String },

    #[error(transparent)]
    Build(#[from] BuildOperationError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

/// Executes the build command, compiling canisters defined in the project manifest.
pub(crate) async fn exec(ctx: &Context, args: &BuildArgs) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be built.
    let p = ctx.project.load().await.context("failed to load project")?;

    let (path, canister) =
        p.canisters
            .get(&args.name)
            .ok_or_else(|| CommandError::CanisterNotFound {
                name: args.name.clone(),
            })?;

    build_many_with_progress_bar(
        vec![(path.clone(), canister.clone())],
        ctx.builder.clone(),
        ctx.artifacts.clone(),
        &ctx.term,
        ctx.debug,
    )
    .await?;

    Ok(())
}
