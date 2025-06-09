use clap::Parser;
use icp_adapter::{Adapter as _, AdapterCompileError};
use icp_canister::model::Adapter;
use icp_project::{
    directory::{FindProjectError, ProjectDirectory},
    model::{CanistersField, LoadProjectManifestError, ProjectManifest},
};
use snafu::Snafu;

#[derive(Parser, Debug)]
pub struct Cmd;

/// Executes the build command, compiling canisters defined in the project manifest.
///
/// This function performs the following steps:
/// 1. Locates the ICP project directory.
/// 2. Loads the project manifest (`icp.yaml`), which can define either a single
///    canister or multiple canisters using glob patterns.
/// 3. Normalizes the canister definitions into a unified list.
/// 4. Iterates through each defined canister and invokes its respective build adapter
///    (Rust, Motoko, or custom script) to compile it into WebAssembly.
pub async fn exec(_: Cmd) -> Result<(), BuildCommandError> {
    // Find the current ICP project directory.
    let pd = ProjectDirectory::find()?.ok_or(BuildCommandError::ProjectNotFound)?;

    // Get the project directory structure for path resolution.
    let pds = pd.structure();

    // Load the project manifest, which defines the canisters to be built.
    let pm = ProjectManifest::load(pds)?;

    // Normalize the `CanistersField` into a flat list of (path, CanisterManifest) tuples.
    // This handles both single-canister and multi-canister configurations uniformly.
    let cs = match pm.canisters {
        // If a single canister was defined, wrap it in a vector.
        CanistersField::Canister((path, c)) => vec![(path, c)],

        // If multiple canisters were defined (or default glob applied), use the list directly.
        CanistersField::Canisters(cs) => cs,
    };

    // Iterate through each resolved canister and trigger its build process.
    for (path, c) in cs {
        match c.build.adapter {
            // Compile using the custom script adapter.
            Adapter::Script(adapter) => {
                adapter.compile(&path).await?;
            }

            // Compile using the Motoko adapter.
            Adapter::Motoko(adapter) => {
                adapter.compile(&path).await?;
            }

            // Compile using the Rust adapter.
            Adapter::Rust(adapter) => {
                adapter.compile(&path).await?;
            }
        }
    }

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum BuildCommandError {
    #[snafu(transparent)]
    FindProjectError { source: FindProjectError },

    #[snafu(display("no project (icp.yaml) found in current directory or its parents"))]
    ProjectNotFound,

    #[snafu(transparent)]
    ProjectLoad { source: LoadProjectManifestError },

    #[snafu(transparent)]
    BuildAdapter { source: AdapterCompileError },
}
