use anyhow::{Context as _, anyhow, bail};
use clap::Args;
use icp::{
    identity::manifest::IdentityList,
    network::{Configuration, run_network},
    project::DEFAULT_LOCAL_NETWORK_NAME,
};
use tracing::debug;

use icp::context::Context;

/// Run a given network
#[derive(Args, Debug)]
pub(crate) struct StartArgs {
    /// Name of the network to start
    #[arg(default_value = DEFAULT_LOCAL_NETWORK_NAME)]
    name: String,

    /// Starts the network in a background process. This command will exit once the network is running.
    /// To stop the network, use 'icp network stop'.
    #[arg(short = 'd', long)]
    background: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &StartArgs) -> Result<(), anyhow::Error> {
    // Load project
    let p = ctx.project.load().await?;

    // Obtain network configuration
    let network = p
        .networks
        .get(&args.name)
        .ok_or_else(|| anyhow!("project does not contain a network named '{}'", args.name))?;

    let cfg = &network.configuration;
    match cfg {
        // Non-managed networks cannot be started
        Configuration::Connected { .. } => {
            bail!("network '{}' is not a managed network", args.name)
        }
        Configuration::Managed { .. } | Configuration::ManagedContainer { .. } => {}
    };

    let pdir = &p.dir;

    // Network directory
    let nd = ctx.network.get_network_directory(network)?;
    nd.ensure_exists()
        .context("failed to create network directory")?;

    if nd.load_network_descriptor().await?.is_some() {
        bail!("network '{}' is already running", args.name);
    }

    // Clean up any existing canister ID mappings of which environment is on this network
    for env in p.environments.values() {
        if env.network == *network {
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
    )
    .await?;
    Ok(())
}
