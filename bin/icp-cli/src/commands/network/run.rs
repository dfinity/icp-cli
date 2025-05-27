use crate::project::structure::ProjectStructure;
use clap::Parser;
use icp_network::structure::NetworkDirectoryStructure;
use icp_network::{ManagedNetworkModel, StartLocalNetworkError, run_local_network};
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

pub async fn exec(cmd: Cmd) -> Result<(), RunNetworkError> {
    println!("Running network command");

    let config = ManagedNetworkModel::default();
    let ps = ProjectStructure::find().ok_or(RunNetworkError::ProjectStructureNotFound)?;
    eprintln!("Project structure root: {}", ps.root().display());
    let network_root = ps.network_root("local");
    create_dir_all(&network_root)?;

    eprintln!("Network root: {}", network_root.display());

    let nds = NetworkDirectoryStructure::new(&network_root);
    run_local_network(config, nds).await?;

    Ok(())
}
