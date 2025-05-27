use crate::commands::network::run::RunNetworkCommandError::ProjectStructureNotFound;
use crate::project::structure::ProjectDirectoryStructure;
use clap::Parser;
use icp_network::{ManagedNetworkModel, RunNetworkError, run_network};
use snafu::Snafu;

/// Run the local network
#[derive(Parser, Debug)]
pub struct Cmd {}

pub async fn exec(_cmd: Cmd) -> Result<(), RunNetworkCommandError> {
    let config = ManagedNetworkModel::default();
    let ps = ProjectDirectoryStructure::find().ok_or(ProjectStructureNotFound)?;
    eprintln!("Project root: {}", ps.root().display());

    let nds = ps.network("local");
    eprintln!("Network root: {}", nds.network_root().display());

    run_network(config, nds).await?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum RunNetworkCommandError {
    #[snafu(display("no project (icp.yaml) found in current directory or its parents"))]
    ProjectStructureNotFound,

    #[snafu(transparent)]
    NetworkExecutionFailed { source: RunNetworkError },
}
