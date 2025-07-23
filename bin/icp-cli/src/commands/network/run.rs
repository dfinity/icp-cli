use crate::context::{Context, GetProjectError};
use clap::Parser;
use icp_network::{NetworkConfig, RunNetworkError, run_network};
use icp_project::project::NoSuchNetworkError;
use snafu::Snafu;

/// Run the local network
#[derive(Parser, Debug)]
pub struct Cmd {}

pub async fn exec(ctx: &Context, _cmd: Cmd) -> Result<(), RunNetworkCommandError> {
    let project = ctx.project()?;
    let pd = &project.directory;
    let network_name = "local";
    let config = project.get_network_config(network_name)?;
    let NetworkConfig::Managed(config) = config else {
        return Err(RunNetworkCommandError::NetworkConfigMustBeManaged {
            network_name: network_name.to_string(),
        });
    };
    let nd = pd.network(network_name, ctx.dirs().port_descriptor_dir());
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
    NoSuchNetwork { source: NoSuchNetworkError },

    #[snafu(transparent)]
    RunNetwork { source: RunNetworkError },

    #[snafu(display("network configuration '{network_name}' must be a managed network"))]
    NetworkConfigMustBeManaged { network_name: String },
}
