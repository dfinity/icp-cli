use std::collections::HashSet;
use std::io;
use std::time::Duration;

use camino_tempfile::tempdir;
use clap::Parser;
use futures::{StreamExt, stream::FuturesOrdered};
use icp_adapter::build::{Adapter as _, AdapterCompileError};
use icp_canister::BuildStep;
use icp_fs::fs::{ReadFileError, read};
use indicatif::{MultiProgress, ProgressBar};
use snafu::{ResultExt, Snafu};

use crate::context::GetProjectError;
use crate::{
    COLOR_FAILURE, COLOR_REGULAR, COLOR_SUCCESS, TICK_EMPTY, TICK_FAILURE, TICK_SUCCESS, make_style,
};
use crate::{context::Context, store_artifact::SaveError};

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

    let mp = MultiProgress::new();

    // Iterate through each resolved canister and trigger its build process.
    for (canister_path, c) in canisters {
        // Attach spinner to multi-progress-bar container
        let pb = mp.add(ProgressBar::new_spinner().with_style(make_style(
            TICK_EMPTY,    // end_tick
            COLOR_REGULAR, // color
        )));

        // Auto-tick spinner
        pb.enable_steady_tick(Duration::from_millis(120));

        // Set the progress bar prefix to display the canister name in brackets
        pb.set_prefix(format!("[{}]", c.name));

        // Create an async closure that handles the build process for this specific canister
        let build_fn = {
            let c = c.clone();
            let pb = pb.clone();

            async move {
                // Create a temporary directory for build artifacts
                let build_dir = tempdir().context(BuildDirSnafu)?;

                // Prepare a path for our output wasm
                let wasm_output_path = build_dir.path().join("out.wasm");

                for step in c.build.steps {
                    // Indicate to user the current step being executed
                    pb.set_message(format!("Building: {step}"));

                    match step {
                        // Compile using the custom script adapter.
                        BuildStep::Script(adapter) => {
                            adapter.compile(&canister_path, &wasm_output_path).await?
                        }

                        // Compile using the Motoko adapter.
                        BuildStep::Motoko(adapter) => {
                            adapter.compile(&canister_path, &wasm_output_path).await?
                        }

                        // Compile using the Rust adapter.
                        BuildStep::Rust(adapter) => {
                            adapter.compile(&canister_path, &wasm_output_path).await?
                        }

                        // Compile using the Pre-built adapter.
                        BuildStep::Prebuilt(adapter) => {
                            adapter.compile(&canister_path, &wasm_output_path).await?
                        }
                    };
                }

                // Verify a file exists in the wasm output path
                if !wasm_output_path.exists() {
                    return Err(CommandError::NoOutput);
                }

                // Load wasm output
                let wasm = read(wasm_output_path).context(ReadOutputSnafu)?;

                // TODO(or.ricon): Verify wasm output is valid wasm (consider using wasmparser)

                // Save the wasm artifact
                ctx.artifact_store.save(&c.name, &wasm)?;

                Ok::<_, CommandError>(())
            }
        };

        futs.push_back(async move {
            // Execute the build function and capture the result
            let out = build_fn.await;

            // Update the progress bar style based on build result
            pb.set_style(match &out {
                Ok(_) => make_style(TICK_SUCCESS, COLOR_SUCCESS),
                Err(_) => make_style(TICK_FAILURE, COLOR_FAILURE),
            });

            // Update the progress bar message based on build result
            pb.set_message(match &out {
                Ok(_) => "Built successfully".to_string(),
                Err(err) => format!("Failed to build canister: {err}"),
            });

            // Stop the progress bar spinner and keep the final state visible
            pb.finish();

            out
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
    GetProject { source: GetProjectError },

    #[snafu(display("project does not contain a canister named '{name}'"))]
    CanisterNotFound { name: String },

    #[snafu(display("failed to create a temporary build directory"))]
    BuildDir { source: io::Error },

    #[snafu(transparent)]
    BuildAdapter { source: AdapterCompileError },

    #[snafu(display("failed to read output wasm artifact"))]
    ReadOutput { source: ReadFileError },

    #[snafu(display("no output has been set"))]
    NoOutput,

    #[snafu(transparent)]
    ArtifactStore { source: SaveError },
}
