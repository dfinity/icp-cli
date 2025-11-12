use anyhow::Context as _;
use clap::Args;
use icp::context::Context;

use crate::operations::build::{BuildOperationError, build_many_with_progress_bar};

#[derive(Args, Debug)]
pub(crate) struct BuildArgs {
    /// The names of the canisters within the current project
    pub(crate) names: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error("project does not contain a canister named '{name}'")]
    CanisterNotFound { name: String },

    #[error(transparent)]
    Build(#[from] BuildOperationError),

    #[error("failed to join build output")]
    JoinError(#[from] tokio::task::JoinError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

/// Executes the build command, compiling canisters defined in the project manifest.
pub(crate) async fn exec(ctx: &Context, args: &BuildArgs) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be built.
    let p = ctx.project.load().await.context("failed to load project")?;

    // Choose canisters to build
    let cnames = match args.names.is_empty() {
        // No canisters specified
        true => p.canisters.keys().cloned().collect(),

        // Individual canisters specified
        false => args.names.clone(),
    };

    for name in &cnames {
        if !p.canisters.contains_key(name) {
            return Err(CommandError::CanisterNotFound {
                name: name.to_owned(),
            });
        }
    }

    let cs = p
        .canisters
        .iter()
        .filter(|(k, _)| cnames.contains(k))
        .map(|(_, (path, canister))| (path.clone(), canister.clone()))
        .collect::<Vec<_>>();

    build_many_with_progress_bar(
        cs,
        ctx.builder.clone(),
        ctx.artifacts.clone(),
        &ctx.term,
        ctx.debug,
    )
    .await?;

    Ok(())
}
