use clap::Parser;
use icp_adapter::{Adapter as _, AdapterCompileError};
use icp_canister::model::{Adapter, CanisterManifest, LoadCanisterManifestError};
use icp_project::{
    directory::{FindProjectError, ProjectDirectory},
    model::{LoadProjectManifestError, ProjectManifest},
};
use snafu::Snafu;

#[derive(Parser, Debug)]
pub struct Cmd;

pub async fn exec(_cmd: Cmd) -> Result<(), BuildCommandError> {
    // Project
    let pd = ProjectDirectory::find()?.ok_or(BuildCommandError::ProjectNotFound)?;

    // Project Structure (paths, etc)
    let pds = pd.structure();

    // Load
    let pm = ProjectManifest::load(pds)?;

    // List canisters in project
    let mut cs = Vec::new();

    for c in pm.canisters {
        let path = pds.canister_yaml_path(&c);
        let cm = CanisterManifest::from_file(&path)?;
        cs.push((path, cm));
    }

    // Build canisters
    for (path, c) in cs {
        match c.build.adapter {
            Adapter::Script(adapter) => {
                adapter.compile(path).await?;
            }

            Adapter::Motoko(adapter) => {
                adapter.compile(path).await?;
            }

            Adapter::Rust(adapter) => {
                adapter.compile(path).await?;
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
    CanisterLoad { source: LoadCanisterManifestError },

    #[snafu(transparent)]
    BuildAdapter { source: AdapterCompileError },
}
