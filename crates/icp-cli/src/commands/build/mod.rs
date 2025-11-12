use std::collections::HashMap;

use anyhow::{Context as _, anyhow};
use clap::Args;
use futures::{StreamExt, stream::FuturesOrdered};
use icp::context::Context;

use crate::{
    operations::build::{BuildOperationError, build},
    progress::{ProgressManager, ProgressManagerSettings},
};

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
        .collect::<HashMap<_, _>>();

    // Prepare a futures set for concurrent canister builds
    let mut futs = FuturesOrdered::new();

    let progress_manager = ProgressManager::new(ProgressManagerSettings { hidden: ctx.debug });

    // Iterate through each resolved canister and trigger its build process.
    for (_, (canister_path, c)) in cs {
        // Create progress bar with standard configuration
        let mut pb = progress_manager.create_multi_step_progress_bar(&c.name, "Build");

        // Create an async closure that handles the build process for this specific canister
        let fut = {
            let c = c.clone();

            async move {
                // Define the build logic
                let build_result = build(
                    canister_path,
                    &c,
                    &mut pb,
                    ctx.builder.clone(),
                    ctx.artifacts.clone(),
                )
                .await;

                // Execute with progress tracking for final state
                let result = ProgressManager::execute_with_progress(
                    &pb,
                    async { build_result },
                    || "Built successfully".to_string(),
                    print_build_error,
                )
                .await;

                // If build failed, get the output for later display
                let output = if result.is_err() {
                    Some(pb.dump_output())
                } else {
                    None
                };

                (result, output)
            }
        };

        futs.push_back(fut);
    }

    // Consume the set of futures and collect results
    let mut failed_outputs = Vec::new();

    while let Some((res, output)) = futs.next().await {
        if let Err(e) = res
            && let Some(output) = output
        {
            failed_outputs.push((e, output));
        }
    }

    // If any builds failed, dump the output and abort
    if !failed_outputs.is_empty() {
        for (e, output) in failed_outputs {
            for line in output {
                let _ = ctx.term.write_line(&line);
            }
            let _ = ctx.term.write_line(&print_build_error(&e));
            let _ = ctx.term.write_line("");
        }

        return Err(CommandError::Unexpected(anyhow!(
            "One or more canisters failed to build"
        )));
    }

    Ok(())
}

fn print_build_error(err: &BuildOperationError) -> String {
    format!("Failed to build canister: {err}")
}
