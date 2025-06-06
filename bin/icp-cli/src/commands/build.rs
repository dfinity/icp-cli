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

pub async fn exec(_: Cmd) -> Result<(), BuildCommandError> {
    // Project
    let pd = ProjectDirectory::find()?.ok_or(BuildCommandError::ProjectNotFound)?;

    // Project Structure (paths, etc)
    let pds = pd.structure();

    // Load
    let pm = ProjectManifest::load(pds)?;

    // Normalize to a list
    let cs = match pm.canisters {
        // Case 1: single-canister
        CanistersField::Canister((path, c)) => vec![(path, c)],

        // Case 2: multi-canister
        CanistersField::Canisters(cs) => cs,
    };

    // Build canisters
    for (path, c) in cs {
        match c.build.adapter {
            Adapter::Script(adapter) => {
                adapter.compile(&path).await?;
            }

            Adapter::Motoko(adapter) => {
                adapter.compile(&path).await?;
            }

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
