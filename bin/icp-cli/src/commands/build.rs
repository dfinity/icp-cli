use clap::Parser;
use icp_canister::{CanisterManifest, CanisterManifestError};
use snafu::{ResultExt, Snafu};

use icp_fs::fs::{ReadFileError, read};
use icp_project::{ProjectManifest, ProjectManifestError};

use crate::project::structure::ProjectDirectoryStructure;

#[derive(Parser, Debug)]
pub struct Cmd;

pub async fn dispatch(_cmd: Cmd) -> Result<(), BuildCommandError> {
    // Open project
    let pds = ProjectDirectoryStructure::find().ok_or(BuildCommandError::ProjectNotFound)?;

    let mpath = pds.root().join("icp.yaml");
    if !mpath.exists() {
        return Err(BuildCommandError::ProjectNotFound);
    }

    let bs = read(mpath)?;
    let pm = ProjectManifest::from_bytes(&bs)?;

    // List canisters in project
    let mut cs = Vec::new();

    for c in pm.canisters {
        let mpath = c.join("canister.yaml");
        if !mpath.exists() {
            return Err(BuildCommandError::CanisterNotFound {
                path: format!("{mpath:?}"),
            });
        }

        let bs = read(mpath)?;
        let cm = CanisterManifest::from_bytes(&bs)?;

        cs.push(cm);
    }

    // Build canisters
    println!("{cs:?}");

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum BuildCommandError {
    #[snafu(display("no project (icp.yaml) found in current directory or its parents"))]
    ProjectNotFound,

    #[snafu(transparent)]
    ProjectParse { source: ProjectManifestError },

    #[snafu(display("canister manifest not found: {path}"))]
    CanisterNotFound { path: String },

    #[snafu(transparent)]
    CanisterParse { source: CanisterManifestError },

    #[snafu(transparent)]
    ReadFile { source: ReadFileError },
}
