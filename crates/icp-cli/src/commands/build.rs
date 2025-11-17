use clap::Args;
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

    #[error("project does not contain a canister named '{name}'")]
    CanisterNotFound { name: String },

    #[error("canister '{canister}' is not in environment '{environment}'")]
    EnvironmentCanister {
        environment: String,
        canister: String,
    },

    #[error(transparent)]
    GetEnvironment(#[from] GetEnvironmentError),

    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub(crate) async fn exec(ctx: &Context, args: &BuildArgs) -> Result<(), CommandError> {
    // Get environment selection
    let environment_selection: EnvironmentSelection = args.environment.clone().into();

    // Load the project manifest
    let p = ctx.project.load().await?;

    // Load target environment
    let env = ctx.get_environment(&environment_selection).await?;

    // Determine which canisters to build
    let cnames = match args.canisters.is_empty() {
        // No canisters specified - build all in environment
        true => env.canisters.keys().cloned().collect(),

        // Individual canisters specified
        false => args.canisters.clone(),
    };

    // Validate all specified canisters exist in project and environment
    for name in &cnames {
        if !p.canisters.contains_key(name) {
            return Err(CommandError::CanisterNotFound {
                name: name.to_owned(),
            });
        }

        if !env.canisters.contains_key(name) {
            return Err(CommandError::EnvironmentCanister {
                environment: env.name.to_owned(),
                canister: name.to_owned(),
            });
        }
    }

    // Skip doing any work if no canisters are targeted
    if cnames.is_empty() {
        return Ok(());
    }

    // Build the selected canisters
    let _ = ctx.term.write_line("Building canisters:");
    let canisters_to_build = p
        .canisters
        .iter()
        .filter(|(k, _)| cnames.contains(k))
        .map(|(_, (path, canister))| (path.clone(), canister.clone()))
        .collect::<Vec<_>>();

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
