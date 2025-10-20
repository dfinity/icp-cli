use clap::Args;
use icp::{
    identity::manifest::{LoadIdentityManifestError, load_identity_list},
    manifest,
    network::{Configuration, NetworkDirectory, RunNetworkError, run_network},
};

use crate::commands::{Context, Mode};

/// Run a given network
#[derive(Args, Debug)]
pub(crate) struct RunArgs {
    /// Name of the network to run
    #[arg(default_value = "local")]
    name: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Locate(#[from] manifest::LocateError),

    #[error("project does not contain a network named '{name}'")]
    Network { name: String },

    #[error("network '{name}' must be a managed network")]
    Unmanaged { name: String },

    #[error(transparent)]
    Identities(#[from] LoadIdentityManifestError),

    #[error(transparent)]
    RunNetwork(#[from] RunNetworkError),
}

pub(crate) async fn exec(ctx: &Context, args: &RunArgs) -> Result<(), CommandError> {
    match &ctx.mode {
        Mode::Global => {
            unimplemented!("global mode is not implemented yet");
        }

        Mode::Project(pdir) => {
            // Load project
            let p = ctx.project.load(pdir).await?;

            // Obtain network configuration
            let network = p.networks.get(&args.name).ok_or(CommandError::Network {
                name: args.name.to_owned(),
            })?;

            let cfg = match &network.configuration {
                // Locally-managed network
                Configuration::Managed(cfg) => cfg,

                // Non-managed networks cannot be started
                Configuration::Connected(_) => {
                    return Err(CommandError::Unmanaged {
                        name: args.name.to_owned(),
                    });
                }
            };

            // Network root
            let ndir = pdir.join(".icp").join("networks").join(&network.name);

            // Network directory
            let nd = NetworkDirectory::new(
                &network.name,               // name
                &ndir,                       // network_root
                &ctx.dirs.port_descriptor(), // port_descriptor_dir
            );

            // Identities
            let ids = load_identity_list(&ctx.dirs.identity())?;

            // Determine ICP accounts to seed
            let seed_accounts = ids.identities.values().map(|id| id.principal());

            eprintln!("Project root: {pdir}");
            eprintln!("Network root: {ndir}");

            run_network(
                cfg,           // config
                nd,            // nd
                pdir,          // project_root
                seed_accounts, // seed_accounts
            )
            .await?;
        }
    };

    Ok(())
}
