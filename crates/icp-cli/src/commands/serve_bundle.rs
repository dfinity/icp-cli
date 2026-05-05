use std::sync::{Arc, OnceLock};

use anyhow::{Context as _, bail};
use camino_tempfile::Utf8TempDir;
use clap::Args;
use flate2::read::GzDecoder;
use icp::{
    fs::remove_file,
    identity::manifest::{IdentityDefaults, IdentityList},
    network::{
        Configuration, ManagedMode,
        managed::cache::{
            check_launcher_update_available, download_launcher_version,
            get_cached_launcher_version_if_fresh,
        },
        managed::run::stop_network,
        run_network,
    },
    prelude::*,
    settings::Settings,
    signal::stop_signal,
};
use tar::Archive;
use tracing::{debug, info, warn};

use crate::{
    artifacts,
    commands::deploy::{self, DeployArgs},
    progress::{ProgressManager, ProgressManagerSettings},
};
use icp::context::Context;

/// Extract a project bundle, start a local network, deploy, and print URLs.
///
/// Extracts the bundle to a temporary directory, starts a local managed network,
/// deploys all canisters (including processing `icp_customize.yaml` if present),
/// and prints the URLs. Shuts down and removes the temporary directory on Ctrl-C.
#[derive(Args, Debug)]
#[command(after_long_help = "\
Examples:

    # Serve a bundle interactively
    icp serve-bundle mybundle.tar.gz

    # Skip icp_customize.yaml prompts
    icp serve-bundle mybundle.tar.gz --yes
")]
pub(crate) struct ServeBundleArgs {
    /// Path to the bundle archive (.tar.gz).
    pub(crate) bundle: PathBuf,

    /// Skip confirmation prompts, including icp_customize.yaml prompts.
    #[arg(long, short)]
    pub(crate) yes: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &ServeBundleArgs) -> Result<(), anyhow::Error> {
    // Extract bundle to a temporary directory.
    let temp_dir = extract_bundle(&args.bundle)?;
    let project_root = temp_dir.path().to_path_buf();
    info!("Extracted bundle to {project_root}");

    // Build a context rooted at the extracted directory.
    let bundle_ctx = icp::context::with_project_root_override(ctx, project_root)?;

    // Load the project so we can pick up the managed network config.
    let p = bundle_ctx.project.load().await?;

    let env_selection = icp::context::EnvironmentSelection::Default;
    let env = bundle_ctx.get_environment(&env_selection).await?;
    let network = env.network.clone();

    let cfg = match &network.configuration {
        Configuration::Managed { managed: cfg } => cfg.clone(),
        Configuration::Connected { .. } => {
            bail!(
                "the bundle's default environment uses a connected network '{}', \
                 which cannot be started locally",
                network.name
            )
        }
    };

    let nd = bundle_ctx.network.get_network_directory(&network)?;
    nd.ensure_exists()
        .context("failed to create network directory")?;

    // Warn and clean up if a stale descriptor exists.
    if let Some(descriptor) = nd.load_network_descriptor().await? {
        if descriptor.child_locator.is_alive().await {
            bail!("network '{}' is already running", network.name);
        } else {
            warn!(
                "Found stale network descriptor for '{}' (process is no longer running). \
                 Cleaning up and starting fresh.",
                network.name
            );
            nd.cleanup_port_descriptor(descriptor.gateway_port())
                .await?;
            nd.cleanup_project_network_descriptor().await?;
        }
    }

    // Clean up canister ID mappings for environments on this network.
    for env in p.environments.values() {
        if env.network == network {
            bundle_ctx.ids.cleanup(true, env.name.as_str())?;
        }
    }

    // Load identities.
    let (ids, defaults) = bundle_ctx
        .dirs
        .identity()?
        .with_read(async |dirs| {
            let ids = IdentityList::load_from(dirs)?;
            let defaults = IdentityDefaults::load_from(dirs)?;
            Ok::<_, anyhow::Error>((ids, defaults))
        })
        .await??;

    let all_identities: Vec<candid::Principal> = ids
        .identities
        .values()
        .filter_map(|id| id.principal())
        .collect();

    let default_identity = ids
        .identities
        .get(&defaults.default)
        .and_then(|id| id.principal());

    let candid_ui_wasm = artifacts::get_candid_ui_wasm();
    let proxy_wasm = artifacts::get_proxy_wasm();

    let settings = bundle_ctx
        .dirs
        .settings()?
        .with_read(async |dirs| Settings::load_from(dirs))
        .await??;

    let autocontainerize = cfg!(windows) || settings.autocontainerize;
    let debug = bundle_ctx.debug;

    let network_launcher_path = if let Ok(var) = std::env::var("ICP_CLI_NETWORK_LAUNCHER_PATH") {
        debug!("Network launcher path overridden by ICP_CLI_NETWORK_LAUNCHER_PATH={var}");
        Some(PathBuf::from(var))
    } else if !autocontainerize && let ManagedMode::Launcher(managed_cfg) = &cfg.mode {
        let version = managed_cfg.version.as_deref().unwrap_or("latest");
        let client = reqwest::Client::new();
        bundle_ctx
            .dirs
            .package_cache()?
            .with_write(async |pkg| {
                if let Some((resolved, path)) =
                    get_cached_launcher_version_if_fresh(pkg.read(), version)?
                {
                    if let Some(update) =
                        check_launcher_update_available(pkg, &resolved, &client).await
                    {
                        info!(
                            "A newer network launcher version is available: {update} \
                                 (current: {resolved}). Run `icp network update` to update."
                        );
                    }
                    anyhow::Ok(Some(path))
                } else {
                    debug!("Downloading icp-cli-network-launcher version `{version}`");
                    let progress_manager =
                        ProgressManager::new(ProgressManagerSettings { hidden: debug });
                    let pb = progress_manager.create_independent_progress_bar();
                    pb.set_message(format!("Downloading icp-cli-network-launcher {version}..."));
                    let version_slot: Arc<OnceLock<String>> = Arc::new(OnceLock::new());
                    let version_capture = version_slot.clone();
                    let path = ProgressManager::execute_with_progress(
                        &pb,
                        async {
                            let (ver, path) =
                                download_launcher_version(pkg, version, &client).await?;
                            let _ = version_capture.set(ver);
                            anyhow::Ok(path)
                        },
                        move || {
                            let ver = version_slot.get().map(String::as_str).unwrap();
                            format!("Downloaded icp-cli-network-launcher {ver}")
                        },
                        |err| format!("Failed to download icp-cli-network-launcher: {err}"),
                    )
                    .await?;
                    Ok(Some(path))
                }
            })
            .await??
    } else {
        None
    };

    // Start the network in the background so we can deploy before waiting.
    run_network(
        &cfg,
        nd,
        temp_dir.path(),
        all_identities,
        default_identity,
        Some(candid_ui_wasm),
        Some(proxy_wasm),
        true, // background
        debug,
        network_launcher_path.as_deref(),
        autocontainerize,
    )
    .await?;

    // Deploy all canisters.
    let deploy_args = DeployArgs {
        names: vec![],
        mode: "auto".to_string(),
        subnet: None,
        proxy: None,
        controller: vec![],
        cycles: icp::parsers::CyclesAmount::from(
            crate::commands::canister::create::DEFAULT_CANISTER_CYCLES,
        ),
        yes: args.yes,
        identity: Default::default(),
        environment: Default::default(),
        json: false,
        args_opt: Default::default(),
    };
    deploy::exec(&bundle_ctx, &deploy_args).await?;

    // Block in the foreground until Ctrl-C or SIGTERM.
    info!("Press Ctrl-C to stop the network and clean up.");
    stop_signal().await;
    info!("Shutting down...");

    // Stop the network gracefully.
    stop_bundle_network(&bundle_ctx, &network).await?;

    // temp_dir is dropped here, removing the extracted bundle directory.
    drop(temp_dir);
    info!("Cleaned up temporary directory.");

    Ok(())
}

/// Stops the managed network associated with `network` inside `ctx`.
async fn stop_bundle_network(ctx: &Context, network: &icp::Network) -> Result<(), anyhow::Error> {
    let nd = ctx.network.get_network_directory(network)?;

    let descriptor = match nd.load_network_descriptor().await? {
        Some(d) => d,
        None => {
            warn!(
                "Network '{}' does not appear to be running; skipping stop.",
                network.name
            );
            return Ok(());
        }
    };

    stop_network(&descriptor.child_locator).await?;

    nd.root()?
        .with_write(async |root| {
            let _ = remove_file(&root.network_descriptor_path());
            Ok::<_, anyhow::Error>(())
        })
        .await??;

    Ok(())
}

/// Extracts a `.tar.gz` bundle to a fresh temporary directory.
fn extract_bundle(bundle_path: &Path) -> Result<Utf8TempDir, anyhow::Error> {
    let file = std::fs::File::open(bundle_path.as_std_path())
        .with_context(|| format!("failed to open bundle '{bundle_path}'"))?;
    let gz = GzDecoder::new(file);
    let mut archive = Archive::new(gz);

    let temp_dir = Utf8TempDir::new().context("failed to create temporary directory")?;
    archive
        .unpack(temp_dir.path().as_std_path())
        .with_context(|| format!("failed to extract bundle '{bundle_path}'"))?;

    Ok(temp_dir)
}
