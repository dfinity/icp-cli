use crate::project::structure::ProjectDirectoryStructure;
use clap::Parser;
use icp_network::structure::NetworkDirectoryStructure;
use icp_network::{ManagedNetworkModel, StartLocalNetworkError, run_network};
use icp_fs::fs::{CreateDirAllError, create_dir_all};
use snafu::Snafu;

#[derive(Parser, Debug)]
pub struct Cmd {}

#[derive(Debug, Snafu)]
pub enum RunNetworkError {
    #[snafu(display("Could not determine project structure"))]
    ProjectStructureNotFound,

    #[snafu(transparent)]
    NetworkExecutionFailed { source: StartLocalNetworkError },
}

pub async fn exec(_cmd: Cmd) -> Result<(), RunNetworkError> {
    let config = ManagedNetworkModel::default();
    let ps = ProjectDirectoryStructure::find().ok_or(RunNetworkError::ProjectStructureNotFound)?;
    eprintln!("Project root: {}", ps.root().display());

    let nds = ps.network("local");
    eprintln!("Network root: {}", nds.network_root().display());

    run_network(config, nds).await?;

    Ok(())
}
