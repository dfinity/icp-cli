use clap::Parser;
use icp::identity::{self, manifest::load_identity_list};
use icp_network::{NETWORK_LOCAL, NetworkConfig, RunNetworkError, run_network};

use crate::commands::Context;

/// Run a given network
#[derive(Parser, Debug)]
pub struct Cmd {
    /// Name of the network to run
    #[arg(default_value = NETWORK_LOCAL)]
    name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error(transparent)]
    RunNetwork { source: RunNetworkError },

    #[error("network configuration '{network_name}' must be a managed network")]
    NetworkConfigMustBeManaged { network_name: String },
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load project
    let project = ctx.project.load().await?;

    let identities = load_identity_list(&ctx.dirs.identity())?;

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
        &cmd.name,                    // network_name
        ctx.dirs().port_descriptor(), // port_descriptor
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
