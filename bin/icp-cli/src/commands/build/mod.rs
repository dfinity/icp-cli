use std::collections::HashSet;
use std::io;
use std::sync::Arc;

use camino_tempfile::tempdir;
use clap::Parser;
use futures::{StreamExt, stream::FuturesOrdered};
use icp_adapter::build::{Adapter as _, AdapterCompileError};
use icp_canister::BuildStep;
use icp_fs::fs::{ReadFileError, read};
use snafu::{ResultExt, Snafu};
use tracing::debug;

use crate::context::GetProjectError;
use crate::progress::ProgressManager;
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

    //Create the mulitprogressbar
    //We might choose to disable it depending on the state
    let progress_manager = ProgressManager::new();

    // Iterate through each resolved canister and trigger its build process.
    for (canister_path, c) in canisters {
        // Create an async closure that handles the build process for this specific canister
        debug!("Building {}", c.name);
        let build_fn = {
            let c = c.clone();
            let sph = Arc::new(progress_manager.new_progress_handler(c.name.clone()));

            async move {
                // Create a temporary directory for build artifacts
                let build_dir = tempdir().context(BuildDirSnafu)?;

                // Prepare a path for our output wasm
                let wasm_output_path = build_dir.path().join("out.wasm");

                let _step_count = c.build.steps.len();
                for (i, step) in c.build.steps.iter().enumerate() {
                    // Indicate to user the current step being executed
                    let _current_step = i + 1;

                    let sph = sph.clone();

                    match step {
                        // Compile using the custom script adapter.
                        BuildStep::Script(adapter) => {
                            // Setup script progress handling

                            adapter
                                .with_progress_handler(sph) 
                                .compile(&canister_path, &wasm_output_path).await?
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

        futs.push_back(build_fn);
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
