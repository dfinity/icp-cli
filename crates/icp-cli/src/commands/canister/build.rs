use anyhow::anyhow;
use clap::Args;
use icp::context::{CanisterSelection, Context};

use crate::operations::build::{BuildOperationError, build_many_with_progress_bar};

#[derive(Debug, Args)]
pub(crate) struct BuildArgs {
    #[command(flatten)]
    pub(crate) cmd_args: crate::commands::args::CanisterCommandArgs,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    BuildOperation(#[from] BuildOperationError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub(crate) async fn exec(ctx: &Context, args: &BuildArgs) -> Result<(), CommandError> {
    let selections = args.cmd_args.selections();
    
    // Extract canister name (reject Principal)
    let canister_name = match selections.canister {
        CanisterSelection::Named(name) => name,
        CanisterSelection::Principal(_) => {
            return Err(anyhow!("Cannot build canister by principal. Please specify a canister name"))?
        }
    };

    // Load the project manifest to get canister info
    let p = ctx.project.load().await.map_err(|e| anyhow!(e))?;

    let (path, canister) = p
        .canisters
        .get(&canister_name)
        .ok_or_else(|| anyhow!("Project does not contain a canister named '{}'", canister_name))?;

    // Build the single canister
    build_many_with_progress_bar(
        vec![(path.clone(), canister.clone())],
        ctx.builder.clone(),
        ctx.artifacts.clone(),
        &ctx.term,
        ctx.debug,
    )
    .await?;

    let _ = ctx
        .term
        .write_line(&format!("Canister {} built successfully", canister_name));

    Ok(())
}

