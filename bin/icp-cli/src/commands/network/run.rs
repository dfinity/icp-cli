use crate::commands::network::run::RunNetworkCommandError::ProjectNotFound;
use clap::Parser;
use icp_network::{ManagedNetworkModel, RunNetworkError, run_network};
use icp_project::directory::{FindProjectError, ProjectDirectory};
use snafu::Snafu;

/// Run the local network
#[derive(Parser, Debug)]
pub struct Cmd {}

pub async fn exec(_cmd: Cmd) -> Result<(), RunNetworkCommandError> {
    let config = ManagedNetworkModel::default();
    let pd = ProjectDirectory::find()?.ok_or(ProjectNotFound)?;
    let nd = pd.network("local");

    eprintln!("Project root: {}", pd.structure().root());
    eprintln!("Network root: {}", nd.structure().network_root());

    run_network(config, nd).await?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum RunNetworkCommandError {
    #[snafu(display("no project (icp.yaml) found in current directory or its parents"))]
    ProjectNotFound,

    #[snafu(transparent)]
    FindProjectError { source: FindProjectError },

    #[snafu(transparent)]
    NetworkExecutionFailed { source: RunNetworkError },
}
