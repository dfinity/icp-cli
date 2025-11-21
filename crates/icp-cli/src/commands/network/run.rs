use clap::Args;
use ic_agent::AgentError;
use icp::{
    fs::lock::LockError,
    identity::manifest::{IdentityList, LoadIdentityManifestError},
    manifest,
    network::{Configuration, RunNetworkError, run_network},
    project::DEFAULT_LOCAL_NETWORK_NAME,
};
use tracing::debug;

use icp::context::Context;

/// Run a given network
#[derive(Args, Debug)]
pub(crate) struct RunArgs {
    /// Name of the network to run
    #[arg(default_value = DEFAULT_LOCAL_NETWORK_NAME)]
    name: String,

    /// Starts the network in a background process. This command will exit once the network is running.
    /// To stop the network, use 'icp network stop'.
    #[arg(long)]
    background: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Locate(#[from] manifest::ProjectRootLocateError),

    #[error(transparent)]
    Agent(#[from] AgentError),

    #[error("project does not contain a network named '{name}'")]
    Network { name: String },

    #[error("network '{name}' must be a managed network")]
    Unmanaged { name: String },

    #[error("failed to create network directory")]
    CreateNetworkDir { source: icp::fs::Error },

    #[error(transparent)]
    LoadNetworkDescriptor(#[from] icp::network::directory::LoadNetworkFileError),

    #[error("network '{name}' is already running")]
    AlreadyRunning { name: String },

    #[error("failed to cleanup canister ID store for environment '{env}'")]
    CleanupCanisterIdStore {
        source: icp::store_id::CleanupError,
        env: String,
    },

    #[error(transparent)]
    NetworkAccess(#[from] icp::network::AccessError),

    #[error(transparent)]
    Identities(#[from] LoadIdentityManifestError),

    #[error(transparent)]
    RunNetwork(#[from] RunNetworkError),

    #[error(transparent)]
    SavePid(#[from] icp::network::SavePidError),

    #[error(transparent)]
    LoadLock(#[from] LockError),
}

pub(crate) async fn exec(ctx: &Context, args: &RunArgs) -> Result<(), CommandError> {
    // Load project
    let p = ctx.project.load().await?;

    // Obtain network configuration
    let network = p.networks.get(&args.name).ok_or(CommandError::Network {
        name: args.name.to_owned(),
    })?;

    let cfg = match &network.configuration {
        // Locally-managed network
        Configuration::Managed { managed: cfg } => cfg,

        // Non-managed networks cannot be started
        Configuration::Connected { connected: _ } => {
            return Err(CommandError::Unmanaged {
                name: args.name.to_owned(),
            });
        }
    };

    let pdir = &p.dir;

    // Network directory
    let nd = ctx.network.get_network_directory(network)?;
    nd.ensure_exists()
        .map_err(|e| CommandError::CreateNetworkDir { source: e })?;

    if nd.load_network_descriptor().await?.is_some() {
        return Err(CommandError::AlreadyRunning {
            name: args.name.to_owned(),
        });
    }

    // Clean up any existing canister ID mappings of which environment is on this network
    for env in p.environments.values() {
        if env.network == *network {
            // It's been ensured that the network is managed, so is_cache is true.
            ctx.ids.cleanup(true, env.name.as_str()).map_err(|e| {
                CommandError::CleanupCanisterIdStore {
                    source: e,
                    env: env.name.to_owned(),
                }
            })?;
        }
    }

    // Identities
    let ids = ctx
        .dirs
        .identity()?
        .with_read(async |dirs| IdentityList::load_from(dirs))
        .await??;

    // Determine ICP accounts to seed
    let seed_accounts = ids.identities.values().map(|id| id.principal());

    debug!("Project root: {pdir}");
    debug!("Network root: {}", nd.network_root);

    run_network(cfg, nd, pdir, seed_accounts, args.background).await?;
    Ok(())
}
