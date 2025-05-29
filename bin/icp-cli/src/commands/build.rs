use std::path::PathBuf;

use clap::Parser;
use icp_canister::{CanisterManifest, CanisterManifestError};
use snafu::{ResultExt, Snafu};

use icp_fs::fs::{ReadFileError, read};
use icp_project::{ProjectDirectoryStructure, ProjectManifest, ProjectManifestError};

#[derive(Parser, Debug)]
pub struct Cmd;

pub async fn dispatch(_cmd: Cmd) -> Result<(), BuildCommandError> {
    // Open project
    let pds = ProjectDirectoryStructure::find().ok_or(BuildCommandError::ProjectNotFound)?;

    let mpath = pds.root().join("icp.yaml");
    if !mpath.exists() {
        return Err(BuildCommandError::ProjectNotFound);
    }

    let bs = read(mpath).context(ProjectLoadSnafu)?;
    let pm = ProjectManifest::from_bytes(&bs).context(ProjectParseSnafu)?;

    println!("{pm:?}");

    // List canisters in project
    let mut cs = Vec::new();

    for c in pm.canisters {
        let mpath = c.join("canister.yaml");
        if !mpath.exists() {
            return Err(BuildCommandError::CanisterNotFound {
                path: format!("{mpath:?}"),
            });
        }

        let bs = read(mpath).context(CanisterLoadSnafu)?;
        let cm = CanisterManifest::from_bytes(&bs).context(CanisterParseSnafu)?;

        cs.push(cm);
    }

    // Build canisters

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum BuildCommandError {
    #[snafu(display("no project (icp.yaml) found in current directory or its parents"))]
    ProjectNotFound,

    #[snafu(display("failed to load project manifest: {source}"))]
    ProjectLoad { source: ReadFileError },

    #[snafu(display("failed to parse project manifest: {source}"))]
    ProjectParse { source: ProjectManifestError },

    #[snafu(display("canister manifest not found: {path}"))]
    CanisterNotFound { path: String },

    #[snafu(display("failed to load canister manifest: {source}"))]
    CanisterLoad { source: ReadFileError },

    #[snafu(display("failed to parse canister manifest: {source}"))]
    CanisterParse { source: CanisterManifestError },
}
