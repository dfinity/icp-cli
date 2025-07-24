use crate::context::{Context, GetProjectError};
use clap::Parser;
use icp_network::{NetworkConfig, RunNetworkError, run_network};
use icp_project::project::NoSuchNetworkError;
use snafu::Snafu;

/// Run the local network
#[derive(Parser, Debug)]
pub struct Cmd {
    /// Name of the network to run
    #[clap(default_value = "local")]
    name: String,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), RunNetworkCommandError> {
    // Load project
    let project = ctx.project()?;

    // Obtain network configuration
    let cfg = match project.get_network_config(&cmd.name)? {
        // Locally-managed network
        NetworkConfig::Managed(cfg) => cfg,

        // Non-managed networks cannot be started
        NetworkConfig::Connected(_) => {
            return Err(RunNetworkCommandError::NetworkConfigMustBeManaged {
                network_name: cmd.name,
            });
        }
    };

    // Project directory
    let pd = &project.directory;

    // Network directory
    let nd = pd.network(
        &cmd.name,                        // network_name
        ctx.dirs().port_descriptor_dir(), // port_descriptor
    );

    eprintln!("Project root: {}", pd.structure().root());
    eprintln!("Network root: {}", nd.structure().network_root);

    run_network(
        cfg,                   // config
        nd,                    // nd
        pd.structure().root(), // project_root
    )
    .await?;

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
