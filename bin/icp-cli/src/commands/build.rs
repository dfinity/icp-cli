use std::path::PathBuf;

use clap::Parser;
use snafu::{ResultExt, Snafu};

use icp_canister::{CanisterManifest, LoadCanisterManifestError};
use icp_project::{LoadProjectManifestError, ProjectManifest};

use crate::project::directory::ProjectDirectory;

#[derive(Parser, Debug)]
pub struct Cmd;

pub async fn dispatch(_cmd: Cmd) -> Result<(), BuildCommandError> {
    let path = ProjectDirectory::find()
        .ok_or(BuildCommandError::ProjectNotFound)?
        .structure()
        .project_yaml_path();

    let pm = ProjectManifest::from_file(&path).context(ProjectLoadSnafu { path })?;

    // List canisters in project
    let mut cs = Vec::new();

    for c in pm.canisters {
        let path = c.join("canister.yaml");

        let cm = CanisterManifest::from_file(&path).context(CanisterLoadSnafu { path })?;

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

    #[snafu(display("failed to load project manifest: {}", path.display()))]
    ProjectLoad {
        source: LoadProjectManifestError,
        path: PathBuf,
    },

    #[snafu(display("failed to load canister manifest: {}", path.display()))]
    CanisterLoad {
        source: LoadCanisterManifestError,
        path: PathBuf,
    },
}
