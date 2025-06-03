use clap::Parser;
use icp_canister::model::{CanisterManifest, LoadCanisterManifestError};
use icp_project::directory::{FindProjectError, ProjectDirectory};
use icp_project::{LoadProjectManifestError, ProjectManifest};
use snafu::Snafu;

#[derive(Parser, Debug)]
pub struct Cmd;

pub async fn exec(_cmd: Cmd) -> Result<(), BuildCommandError> {
    // Project
    let pd = ProjectDirectory::find()?.ok_or(BuildCommandError::ProjectNotFound)?;

    // Project Structure (paths, etc)
    let pds = pd.structure();

    // Load
    let pm = ProjectManifest::from_file(pds.project_yaml_path())?;

    // List canisters in project
    let mut cs = Vec::new();

    for c in pm.canisters {
        let cm = CanisterManifest::from_file(pds.canister_yaml_path(&c))?;
        cs.push(cm);
    }

    // Build canisters
    println!("{cs:?}");

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
}
