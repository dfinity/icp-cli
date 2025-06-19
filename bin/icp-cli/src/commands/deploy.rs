use crate::env::Env;
use clap::Parser;
use icp_project::{directory::FindProjectError, model::LoadProjectManifestError};
use snafu::Snafu;

#[derive(Parser, Debug)]
pub struct Cmd;

pub async fn exec(_env: &Env, _: Cmd) -> Result<(), DeployCommandError> {
    Ok(())
}

#[derive(Debug, Snafu)]
pub enum DeployCommandError {
    #[snafu(transparent)]
    FindProjectError { source: FindProjectError },

    #[snafu(display("no project (icp.yaml) found in current directory or its parents"))]
    ProjectNotFound,

    #[snafu(transparent)]
    ProjectLoad { source: LoadProjectManifestError },
}
