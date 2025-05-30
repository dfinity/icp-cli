use clap::Parser;
use snafu::Snafu;

use icp_canister::{CanisterManifest, LoadCanisterManifestError};
use icp_project::{LoadProjectManifestError, ProjectManifest};

use crate::project::directory::{FindProjectError, ProjectDirectory};

#[derive(Parser, Debug)]
pub struct Cmd;

pub async fn dispatch(_cmd: Cmd) -> Result<(), BuildCommandError> {
    let path = ProjectDirectory::find()?
        .ok_or(BuildCommandError::ProjectNotFound)?
        .structure()
        .project_yaml_path();

    let pm = ProjectManifest::from_file(&path)?;

    // List canisters in project
    let mut cs = Vec::new();

    for c in pm.canisters {
        let path = c.join("canister.yaml");

        let cm = CanisterManifest::from_file(&path)?;

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
