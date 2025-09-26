use clap::Parser;
use icp_identity::manifest::load_identity_list;
use icp_network::{NETWORK_LOCAL, NetworkConfig, RunNetworkError, run_network};
use icp_project::NoSuchNetworkError;
use snafu::Snafu;

use crate::context::{Context, ContextProjectError};

/// Run a given network
#[derive(Parser, Debug)]
pub struct Cmd {
    /// Name of the network to run
    #[arg(default_value = NETWORK_LOCAL)]
    name: String,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load project
    let project = ctx.project()?;
    let dirs = ctx.dirs();
    let identities = load_identity_list(dirs)?;

    // Obtain network configuration
    let cfg = match project.get_network_config(&cmd.name)? {
        // Locally-managed network
        NetworkConfig::Managed(cfg) => cfg,

        // Non-managed networks cannot be started
        NetworkConfig::Connected(_) => {
            return Err(CommandError::NetworkConfigMustBeManaged {
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

    // Determine ICP accounts to seed
    let seed_accounts = identities.identities.values().map(|id| id.principal());

    eprintln!("Project root: {}", pd.structure().root());
    eprintln!("Network root: {}", nd.structure().network_root);

    run_network(
        cfg,                   // config
        nd,                    // nd
        pd.structure().root(), // project_root
        seed_accounts,         // seed_accounts
    )
    .await?;

    Ok(())
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetProject { source: ContextProjectError },

    #[snafu(transparent)]
    LoadIdentity {
        source: icp_identity::manifest::LoadIdentityManifestError,
    },

    #[snafu(transparent)]
    NoSuchNetwork { source: NoSuchNetworkError },

    #[snafu(transparent)]
    RunNetwork { source: RunNetworkError },

    #[snafu(display("network configuration '{network_name}' must be a managed network"))]
    NetworkConfigMustBeManaged { network_name: String },
}
