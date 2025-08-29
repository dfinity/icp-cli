use std::io;
use std::time::Duration;

use camino_tempfile::tempdir;
use clap::Parser;
use futures::{StreamExt, stream::FuturesOrdered};
use icp_adapter::build::{Adapter as _, AdapterCompileError};
use icp_canister::BuildStep;
use icp_fs::fs::{ReadFileError, read};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use snafu::{ResultExt, Snafu};
use tokio::time::sleep;

use crate::context::GetProjectError;
use crate::{context::Context, store_artifact::SaveError};

//
const TICKS: &[&str] = &["✶", "✸", "✹", "✺", "✹", "✷"];

//
const TICK_EMPTY: &str = " ";
const TICK_SUCCESS: &str = "✔";
const TICK_FAILURE: &str = "✘";

//
const COLOR_REGULAR: &str = "blue";
const COLOR_SUCCESS: &str = "green";
const COLOR_FAILURE: &str = "red";

fn make_style(end_tick: &str, color: &str) -> ProgressStyle {
    let tmpl = format!("{{spinner:.{color}}} {{msg}}");

    ProgressStyle::with_template(&tmpl)
        .expect("invalid style template")
        .tick_strings(&[TICKS, &[end_tick]].concat())
}

#[derive(Parser, Debug)]
pub struct Cmd {
    /// The name of the canister within the current project
    pub name: Option<String>,
}

/// Executes the build command, compiling canisters defined in the project manifest.
///
/// This function performs the following steps:
/// 1. Locates the ICP project directory.
/// 2. Loads the project manifest (`icp.yaml`), which can define either a single
///    canister or multiple canisters using glob patterns.
/// 3. Normalizes the canister definitions into a unified list.
/// 4. Iterates through each defined canister and invokes its respective build adapter
///    (Rust, Motoko, or custom script) to compile it into WebAssembly.
pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be built.
    let pm = ctx.project()?;

    // Choose canisters to build
    let canisters = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.name {
            Some(name) => name == &c.name,
            None => true,
        })
        .cloned()
        .collect::<Vec<_>>();

    // Check if selected canister exists
    if let Some(name) = cmd.name {
        if canisters.is_empty() {
            return Err(CommandError::CanisterNotFound { name });
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

        // Set progress-bar message
        pb.set_message(format!("Building canister: {}", c.name));

        futs.push_back(async move {
            // Create a temporary directory for build artifacts
            let build_dir = tempdir().context(BuildDirSnafu)?;

            // Prepare a path for our output wasm
            let wasm_output_path = build_dir.path().join("out.wasm");

            for step in c.build.steps {
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

            //
            pb.set_style(make_style(
                TICK_SUCCESS,  // end_tick
                COLOR_SUCCESS, // color
            ));

            pb.finish_with_message(format!("Built canister: {}", c.name));

            Ok::<_, CommandError>(())
        });
    }

    // Consume the set of futures and abort if an error occurs
    while let Some(res) = futs.next().await {
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
