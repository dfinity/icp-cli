use std::collections::HashMap;

use anyhow::Context as _;
use camino_tempfile::tempdir;
use clap::Parser;
use futures::{StreamExt, stream::FuturesOrdered};
use icp::{
    canister::build::{BuildError, Params},
    fs::read,
};

use crate::{
    commands::Context,
    progress::{MAX_LINES_PER_STEP, ProgressManager, RollingLines, ScriptProgressHandler},
};

#[derive(Parser, Debug)]
pub struct Cmd {
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

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),

    #[error("failed to join build output")]
    JoinError(#[from] tokio::task::JoinError),
}

/// Executes the build command, compiling canisters defined in the project manifest.
pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be built.
    let p = ctx.project.load().await.context("failed to load project")?;

    // Choose canisters to build
    let cnames = match cmd.names.is_empty() {
        // No canisters specified
        true => p.canisters.keys().cloned().collect(),

        // Individual canisters specified
        false => cmd.names.clone(),
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

    let progress_manager = ProgressManager::new();

    // Iterate through each resolved canister and trigger its build process.
    for (_, (canister_path, c)) in cs {
        // Create progress bar with standard configuration
        let pb = progress_manager.create_progress_bar(&c.name);

        // Create an async closure that handles the build process for this specific canister
        let build_fn = {
            let c = c.clone();
            let pb = pb.clone();

            async move {
                // Create a temporary directory for build artifacts
                let build_dir =
                    tempdir().context("failed to create a temporary build directory")?;

                // Prepare a path for our output wasm
                let wasm_output_path = build_dir.path().join("out.wasm");

                let step_count = c.build.steps.len();
                let mut step_outputs = vec![];
                for (i, step) in c.build.steps.iter().enumerate() {
                    // Indicate to user the current step being executed
                    let current_step = i + 1;
                    let pb_hdr = format!("\nBuilding: {step} {current_step} of {step_count}");

                    let script_handler = ScriptProgressHandler::new(pb.clone(), pb_hdr.clone());

                    // Setup script progress handling and receiver join handle
                    let (tx, rx) = script_handler.setup_output_handler();

                    // Perform build step
                    let build_result = ctx
                        .builder
                        .build(
                            step, // step
                            &Params {
                                path: canister_path.to_owned(),
                                output: wasm_output_path.to_owned(),
                            },
                            tx,
                        )
                        .await;

                    // Ensure background receiver drains all messages
                    let step_output = rx.await?;
                    step_outputs.push(StepOutput {
                        title: pb_hdr,
                        output: step_output,
                    });

                    if let Err(e) = build_result {
                        dump_build_output(&c.name, step_outputs);
                        return Err(CommandError::Build(e));
                    }
                }

                // Verify a file exists in the wasm output path
                if !wasm_output_path.exists() {
                    dump_build_output(&c.name, step_outputs);
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

#[derive(Debug)]
struct StepOutput {
    title: String,
    output: RollingLines,
}

fn dump_build_output(canister_name: &str, step_outputs: Vec<StepOutput>) {
    let crop_message = if step_outputs.len() == MAX_LINES_PER_STEP {
        format!(" (last {MAX_LINES_PER_STEP} lines)")
    } else {
        String::new()
    };
    println!("Build output for canister {canister_name}{crop_message}:");
    for step_output in step_outputs {
        println!("{}", step_output.title);
        for line in step_output.output.iter() {
            println!("{}", line);
        }
    }
}
