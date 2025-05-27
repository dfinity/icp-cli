use crate::project::structure::ProjectStructure;
use clap::Parser;
use icp_network::structure::NetworkDirectoryStructure;
use icp_network::{ManagedNetworkModel, StartLocalNetworkError, run_network};
use icp_support::fs::{CreateDirAllError, create_dir_all};
use snafu::Snafu;

#[derive(Parser, Debug)]
pub struct Cmd {}

#[derive(Debug, Snafu)]
pub enum RunNetworkError {
    #[snafu(display("Could not determine project structure"))]
    ProjectStructureNotFound,

    #[snafu(transparent)]
    CreateDirFailed { source: CreateDirAllError },

    #[snafu(transparent)]
    NetworkExecutionFailed { source: StartLocalNetworkError },
}

pub async fn exec(_cmd: Cmd) -> Result<(), RunNetworkError> {
    let config = ManagedNetworkModel::default();
    let ps = ProjectStructure::find().ok_or(RunNetworkError::ProjectStructureNotFound)?;
    eprintln!("Project root: {}", ps.root().display());

    let nds = ps.network("local");
    eprintln!("Network root: {}", nds.network_root().display());
    create_dir_all(nds.network_root())?;

    run_network(config, nds).await?;

    Ok(())
}
