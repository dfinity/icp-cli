use std::collections::HashSet;
use std::io;

use camino_tempfile::tempdir;
use clap::Parser;
use console::Term;
use futures::{StreamExt, stream::FuturesOrdered};
use icp::fs::read;
use icp_adapter::build::{Adapter as _, AdapterCompileError};
use icp_canister::BuildStep;
use snafu::{ResultExt, Snafu};

use crate::context::ContextProjectError;
use crate::{
    context::Context,
    progress::{ProgressManager, ScriptProgressHandler},
    store_artifact::SaveError,
};

#[derive(Debug)]
struct StepOutput {
    step_description: String,
    lines: Vec<String>,
}

fn dump_build_output(term: &Term, canister_name: &str, steps: Vec<StepOutput>) {
    let _ = term.write_line("");
    let _ = term.write_line(&format!(
        "Build for canister '{}' failed. Build output:",
        canister_name
    ));
    let _ = term.write_line("");

    for step in steps {
        let _ = term.write_line(&step.step_description);
        if step.lines.is_empty() {
            let _ = term.write_line("  (no output)");
        } else {
            for line in step.lines {
                let _ = term.write_line(&format!("  {}", line));
            }
        }
        let _ = term.write_line("");
    }
}

#[derive(Parser, Debug)]
pub struct Cmd {
    /// The names of the canisters within the current project
    pub names: Vec<String>,
}

/// Executes the build command, compiling canisters defined in the project manifest.
pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be built.
    let pm = ctx.project()?;

    // Choose canisters to build
    let canisters = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.names.is_empty() {
            // If no names specified, build all canisters
            true => true,

            // If names specified, only build matching canisters
            false => cmd.names.contains(&c.name),
        })
        .cloned()
        .collect::<Vec<_>>();

    // Check if selected canisters exists
    if !cmd.names.is_empty() {
        let names = canisters
            .iter()
            .map(|(_, c)| &c.name)
            .collect::<HashSet<_>>();

        for name in &cmd.names {
            if !names.contains(name) {
                return Err(CommandError::CanisterNotFound {
                    name: name.to_owned(),
                });
            }
        }
    }

    // Prepare a futures set for concurrent canister builds
    let mut futs = FuturesOrdered::new();

    let progress_manager = ProgressManager::new();

    // Iterate through each resolved canister and trigger its build process.
    for (canister_path, c) in canisters {
        // Create progress bar with standard configuration
        let pb = progress_manager.create_progress_bar(&c.name);

        // Create an async closure that handles the build process for this specific canister
        let build_fn = {
            let c = c.clone();
            let pb = pb.clone();
            let term = ctx.term.clone();

            async move {
                // Create a temporary directory for build artifacts
                let build_dir = tempdir().context(BuildDirSnafu)?;

                // Prepare a path for our output wasm
                let wasm_output_path = build_dir.path().join("out.wasm");

                // Buffer to accumulate output from build steps in case the build fails
                let mut canister_output: Vec<StepOutput> = Vec::new();

                let step_count = c.build.steps.len();
                for (i, step) in c.build.steps.iter().enumerate() {
                    // Indicate to user the current step being executed
                    let current_step = i + 1;
                    let pb_hdr = format!("Building: {step} {current_step} of {step_count}");

                    let script_handler = ScriptProgressHandler::new(pb.clone(), pb_hdr.clone());

                    match step {
                        // Compile using the custom script adapter.
                        BuildStep::Script(adapter) => {
                            // Setup script progress handling and receiver join handle
                            let (tx, rx) = script_handler.setup_output_handler();

                            // Run compile which will feed lines into the channel
                            let result = adapter
                                .with_stdio_sender(tx)
                                .compile(&canister_path, &wasm_output_path)
                                .await;
                            let step_lines = rx.await.context(JoinOutputSnafu)?;
                            canister_output.push(StepOutput {
                                step_description: format!(
                                    "Step {}/{}: {}",
                                    current_step, step_count, step
                                ),
                                lines: step_lines,
                            });

                            if let Err(e) = result {
                                dump_build_output(&term, &c.name, canister_output);
                                return Err(e.into());
                            }
                        }

                        // Compile using the Pre-built adapter.
                        BuildStep::Prebuilt(adapter) => {
                            pb.set_message(pb_hdr.clone());

                            let result = adapter.compile(&canister_path, &wasm_output_path).await;
                            let step_message = match &result {
                                Ok(msg) => msg.clone(),
                                Err(e) => format!("Failed: {}", e),
                            };

                            canister_output.push(StepOutput {
                                step_description: format!(
                                    "Step {}/{}: {}",
                                    current_step, step_count, step
                                ),
                                lines: vec![step_message],
                            });

                            if let Err(e) = result {
                                dump_build_output(&term, &c.name, canister_output);
                                return Err(e.into());
                            }
                        }
                    };
                }

                // Verify a file exists in the wasm output path
                if !wasm_output_path.exists() {
                    dump_build_output(&term, &c.name, canister_output);
                    return Err(CommandError::NoOutput);
                }

                // Load wasm output
                let wasm = read(&wasm_output_path).context(ReadOutputSnafu)?;

                // TODO(or.ricon): Verify wasm output is valid wasm (consider using wasmparser)

                // Save the wasm artifact
                ctx.artifact_store.save(&c.name, &wasm)?;

                Ok::<_, CommandError>(())
            }
        };

        futs.push_back(async move {
            // Execute the build function with progress tracking
            ProgressManager::execute_with_progress(
                pb,
                build_fn,
                || "Built successfully".to_string(),
                |err| format!("Failed to build canister: {err}"),
            )
            .await
        });
    }

    // Consume the set of futures and abort if an error occurs
    while let Some(res) = futs.next().await {
        // TODO(or.ricon): Handle canister build failures
        res?;
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: ContextProjectError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("failed to create a temporary build directory"))]
    BuildDir { source: io::Error },

    #[snafu(transparent)]
    BuildAdapter { source: AdapterCompileError },

    #[snafu(display("failed to read output wasm artifact"))]
    ReadOutput { source: icp::fs::Error },

    #[snafu(display("no output has been set"))]
    NoOutput,

    #[snafu(transparent)]
    ArtifactStore { source: SaveError },

    #[snafu(display("Failed to join output handler thread"))]
    JoinOutput { source: tokio::task::JoinError },
}
