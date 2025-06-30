use std::io;

use crate::env::GetProjectError;
use crate::{env::Env, store_artifact::SaveError};
use camino_tempfile::tempdir;
use clap::Parser;
use icp_adapter::{Adapter as _, AdapterCompileError};
use icp_canister::model::Adapter;
use icp_fs::fs::{ReadFileError, read};
use snafu::{ResultExt, Snafu};

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
pub async fn exec(env: &Env, cmd: Cmd) -> Result<(), CommandError> {
    // Load the project manifest, which defines the canisters to be built.
    let pm = env.project()?;

    // Choose canisters to build
    let canisters = pm
        .canisters
        .iter()
        .filter(|(_, c)| match &cmd.name {
            Some(name) => name == &c.name,
            None => true,
        })
        .collect::<Vec<_>>();

    // Check if selected canister exists
    if let Some(name) = cmd.name {
        if canisters.is_empty() {
            return Err(CommandError::CanisterNotFound { name });
        }
    }

    // Iterate through each resolved canister and trigger its build process.
    for (canister_path, c) in &pm.canisters {
        // Create a temporary directory for build artifacts
        let build_dir = tempdir().context(BuildDirSnafu)?;

        // Prepare a path for our output wasm
        let wasm_output_path = build_dir.path().join("out.wasm");

        match &c.build.adapter {
            // Compile using the custom script adapter.
            Adapter::Script(adapter) => adapter.compile(canister_path, &wasm_output_path).await?,

            // Compile using the Motoko adapter.
            Adapter::Motoko(adapter) => adapter.compile(canister_path, &wasm_output_path).await?,

            // Compile using the Rust adapter.
            Adapter::Rust(adapter) => adapter.compile(canister_path, &wasm_output_path).await?,
        };

        // Verify a file exists in the wasm output path
        if !wasm_output_path.exists() {
            return Err(CommandError::NoOutput);
        }

        // Load wasm output
        let wasm = read(wasm_output_path).context(ReadOutputSnafu)?;

        // TODO(or.ricon): Verify wasm output is valid wasm (consider using wasmparser)

        // Save the wasm artifact
        env.artifact_store.save(&c.name, &wasm)?;
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
