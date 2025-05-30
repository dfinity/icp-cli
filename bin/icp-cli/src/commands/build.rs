use std::path::PathBuf;

use clap::Parser;
use snafu::{ResultExt, Snafu};

use icp_canister::{CanisterManifest, LoadCanisterManifestError};
use icp_project::{LoadProjectManifestError, ProjectManifest};

use crate::project::structure::ProjectDirectoryStructure;

#[derive(Parser, Debug)]
pub struct Cmd;

pub async fn dispatch(_cmd: Cmd) -> Result<(), BuildCommandError> {
    let mpath = ProjectDirectoryStructure::find()
        .ok_or(BuildCommandError::ProjectNotFound)?
        .root()
        .join("icp.yaml");

    let pm = ProjectManifest::from_file(&mpath).context(ProjectLoadSnafu { path: mpath })?;

    // List canisters in project
    let mut cs = Vec::new();

    for c in pm.canisters {
        let mpath = c.join("canister.yaml");

        let cm = CanisterManifest::from_file(&mpath).context(CanisterLoadSnafu { path: mpath })?;

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

    #[snafu(display("failed to load project manifest: {path:?}"))]
    ProjectLoad {
        source: LoadProjectManifestError,
        path: PathBuf,
    },

    #[snafu(display("failed to load canister manifest: {path:?}"))]
    CanisterLoad {
        source: LoadCanisterManifestError,
        path: PathBuf,
    },
}
