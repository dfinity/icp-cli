use std::collections::HashMap;

use anyhow::{Context as _, anyhow};
use camino_tempfile::tempdir;
use clap::Args;
use futures::{StreamExt, stream::FuturesOrdered};
use icp::{
    canister::build::{BuildError, Params},
    fs::read,
};

use crate::{
    commands::{Context, Mode},
    progress::{ProgressManager, ProgressManagerSettings},
};

#[derive(Args, Debug)]
pub struct BuildArgs {
    /// The names of the canisters within the current project
    pub names: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("project does not contain a canister named '{name}'")]
    CanisterNotFound { name: String },

    #[error(transparent)]
    Build(#[from] BuildError),

    #[error("build did not result in output")]
    MissingOutput,

    #[error("failed to read output wasm artifact")]
    ReadOutput,

    #[error("failed to store build artifact")]
    ArtifactStore,

    #[error("failed to join build output")]
    JoinError(#[from] tokio::task::JoinError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

/// Executes the build command, compiling canisters defined in the project manifest.
pub async fn exec(ctx: &Context, args: &BuildArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(_) => {
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

            let progress_manager =
                ProgressManager::new(ProgressManagerSettings { hidden: ctx.debug });

            // Iterate through each resolved canister and trigger its build process.
            for (_, (canister_path, c)) in cs {
                // Create progress bar with standard configuration
                let mut pb = progress_manager.create_multi_step_progress_bar(&c.name, "Build");

                // Create an async closure that handles the build process for this specific canister
                let fut = {
                    let c = c.clone();

                    async move {
                        // Define the build logic
                        let build_result = async {
                            // Create a temporary directory for build artifacts
                            let build_dir = tempdir()
                                .context("failed to create a temporary build directory")?;

                            // Prepare a path for our output wasm
                            let wasm_output_path = build_dir.path().join("out.wasm");

                            let step_count = c.build.steps.len();
                            for (i, step) in c.build.steps.iter().enumerate() {
                                // Indicate to user the current step being executed
                                let current_step = i + 1;
                                let pb_hdr =
                                    format!("Building: step {current_step} of {step_count} {step}");
                                let tx = pb.begin_step(pb_hdr);

                                // Perform build step
                                let build_result = ctx
                                    .builder
                                    .build(
                                        step, // step
                                        &Params {
                                            path: canister_path.to_owned(),
                                            output: wasm_output_path.to_owned(),
                                        },
                                        Some(tx),
                                    )
                                    .await;

                                // Ensure background receiver drains all messages
                                pb.end_step().await;

                                if let Err(e) = build_result {
                                    return Err(CommandError::Build(e));
                                }
                            }

                            // Verify a file exists in the wasm output path
                            if !wasm_output_path.exists() {
                                return Err(CommandError::MissingOutput);
                            }

                            // Load wasm output
                            let wasm = read(&wasm_output_path).context(CommandError::ReadOutput)?;

                            // TODO(or.ricon): Verify wasm output is valid wasm (consider using wasmparser)

                            // Save the wasm artifact
                            ctx.artifacts
                                .save(&c.name, &wasm)
                                .context(CommandError::ArtifactStore)?;

                            Ok::<_, CommandError>(())
                        }
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
                    failed_outputs.push((output, e));
                }
            }

            // If any builds failed, dump the output and abort
            if !failed_outputs.is_empty() {
                for (output, e) in failed_outputs {
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
        }
    }

    Ok(())
}

fn print_build_error(err: &CommandError) -> String {
    format!("Failed to build canister: {err}")
}
