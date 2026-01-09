use anyhow::bail;
use clap::Args;
use icp::{
    fs::remove_file,
    network::{Configuration, config::ChildLocator, managed::run::stop_network},
};

use super::args::NetworkOrEnvironmentArgs;
use icp::context::Context;

/// Stop a background network
///
/// # Examples
///
///   # Stop default 'local' network
///   icp network stop
///
///   # Stop explicit network
///   icp network stop mynetwork
///
///   # Stop using environment flag
///   icp network stop -e staging
///
///   # Stop using ICP_ENVIRONMENT variable
///   ICP_ENVIRONMENT=staging icp network stop
///
///   # Name overrides ICP_ENVIRONMENT
///   ICP_ENVIRONMENT=staging icp network stop local
#[derive(Args, Debug)]
pub struct Cmd {
    #[clap(flatten)]
    network_selection: NetworkOrEnvironmentArgs,
}

pub async fn exec(ctx: &Context, cmd: &Cmd) -> Result<(), anyhow::Error> {
    // Load project
    let _ = ctx.project.load().await?;

    // Convert args to selection and get network
    let selection: Result<_, _> = cmd.network_selection.clone().into();
    let network = ctx.get_network_or_environment(&selection?).await?;

    if let Configuration::Connected { connected: _ } = &network.configuration {
        bail!("network '{}' is not a managed network", network.name)
    };

    // Network directory
    let nd = ctx.network.get_network_directory(&network)?;

    let descriptor = nd
        .load_network_descriptor()
        .await?
        .ok_or_else(|| anyhow::anyhow!("network '{}' is not running", network.name))?;

    match &descriptor.child_locator {
        ChildLocator::Pid { pid } => {
            let _ = ctx
                .term
                .write_line(&format!("Stopping background network (PID: {})...", pid));
        }
        ChildLocator::Container { id, .. } => {
            let _ = ctx.term.write_line(&format!(
                "Stopping background network (container ID: {})...",
                &id[..12]
            ));
        }
    }

    stop_network(&descriptor.child_locator).await?;

    nd.root()?
        .with_write(async |root| {
            let descriptor_file = root.network_descriptor_path();
            // Desciptor file must be deleted to allow the network to be restarted, but if it doesn't exist, that's fine too
            let _ = remove_file(&descriptor_file);

            Ok::<_, anyhow::Error>(())
        })
        .await??;

    let _ = ctx.term.write_line("Network stopped successfully");

    Ok(())
}
