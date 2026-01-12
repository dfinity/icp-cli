use anyhow::{Context as _, bail};
use clap::Args;
use icp::{
    identity::manifest::IdentityList,
    network::{Configuration, run_network},
};
use tracing::debug;

use super::args::NetworkOrEnvironmentArgs;
use icp::context::Context;

/// Run a given network
#[derive(Args, Debug)]
#[command(after_long_help = "\
Examples:
    # Use default 'local' network
    icp network start
  
    # Use explicit network name
    icp network start mynetwork
  
    # Use environment flag
    icp network start -e staging
  
    # Use ICP_ENVIRONMENT variable
    ICP_ENVIRONMENT=staging icp network start
  
    # Name overrides ICP_ENVIRONMENT
    ICP_ENVIRONMENT=staging icp network start local
  
    # Background mode with environment
    icp network start -e staging -d
")]
pub(crate) struct StartArgs {
    #[clap(flatten)]
    network_selection: NetworkOrEnvironmentArgs,

    /// Starts the network in a background process. This command will exit once the network is running.
    /// To stop the network, use 'icp network stop'.
    #[arg(short = 'd', long)]
    background: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &StartArgs) -> Result<(), anyhow::Error> {
    // Load project
    let p = ctx.project.load().await?;

    // Convert args to selection and get network
    let selection: Result<_, _> = args.network_selection.clone().into();
    let network = ctx.get_network_or_environment(&selection?).await?;

    let cfg = match &network.configuration {
        // Locally-managed network
        Configuration::Managed { managed: cfg } => cfg,

        // Non-managed networks cannot be started
        Configuration::Connected { connected: _ } => {
            bail!("network '{}' is not a managed network", network.name)
        }
    };

    let pdir = &p.dir;

    // Network directory
    let nd = ctx.network.get_network_directory(&network)?;
    nd.ensure_exists()
        .context("failed to create network directory")?;

    if nd.load_network_descriptor().await?.is_some() {
        bail!("network '{}' is already running", network.name);
    }

    // Clean up any existing canister ID mappings of which environment is on this network
    for env in p.environments.values() {
        if env.network == network {
            // It's been ensured that the network is managed, so is_cache is true.
            ctx.ids.cleanup(true, env.name.as_str())?;
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

    let candid_ui_wasm = crate::artifacts::get_candid_ui_wasm();

    run_network(
        cfg,
        nd,
        pdir,
        seed_accounts,
        Some(candid_ui_wasm),
        args.background,
        ctx.debug,
    )
    .await?;
    Ok(())
}
