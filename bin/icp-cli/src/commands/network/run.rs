use crate::env::{Env, GetProjectError};
use clap::Parser;
use icp_network::{NetworkConfig, RunNetworkError, run_network};
use snafu::Snafu;

/// Run the local network
#[derive(Parser, Debug)]
pub struct Cmd {}

pub async fn exec(env: &Env, _cmd: Cmd) -> Result<(), RunNetworkCommandError> {
    let project = env.project()?;
    let pd = &project.directory;
    let network_name = "local";
    let config = project.find_network_config(network_name).ok_or_else(|| {
        RunNetworkCommandError::NetworkConfigNotFound {
            network_name: network_name.to_string(),
        }
    })?;
    let NetworkConfig::Managed(config) = config else {
        return Err(RunNetworkCommandError::NetworkConfigMustBeManaged {
            network_name: network_name.to_string(),
        });
    };
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

    #[snafu(display("network configuration '{network_name}' not found"))]
    NetworkConfigNotFound { network_name: String },

    #[snafu(display("network configuration '{network_name}' must be a managed network"))]
    NetworkConfigMustBeManaged { network_name: String },
}
