use crate::env::{Env, GetProjectError};
use clap::Parser;
use icp_network::{ManagedNetworkModel, RunNetworkError, run_network};
use snafu::Snafu;

/// Run the local network
#[derive(Parser, Debug)]
pub struct Cmd {}

pub async fn exec(env: &Env, _cmd: Cmd) -> Result<(), RunNetworkCommandError> {
    let project = env.project()?;
    let pd = &project.directory;
    let network_name = "local";
    let config = ManagedNetworkModel::default();
    let nd = pd.network(network_name, env.dirs().port_descriptor_dir());
    let project_root = pd.structure().root();

    eprintln!("Project root: {project_root}");
    eprintln!("Network root: {}", nd.structure().network_root());

    run_network(config, nd, project_root).await?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum RunNetworkCommandError {
    #[snafu(transparent)]
    GetProject { source: GetProjectError },

    #[snafu(transparent)]
    RunNetwork { source: RunNetworkError },
}
