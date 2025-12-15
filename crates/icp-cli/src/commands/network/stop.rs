use anyhow::{anyhow, bail};
use clap::Parser;
use icp::{
    fs::remove_file,
    network::{Configuration, config::ChildLocator, managed::run::stop_network},
    project::DEFAULT_LOCAL_NETWORK_NAME,
};

use icp::context::Context;

/// Stop a background network
#[derive(Parser, Debug)]
pub struct Cmd {
    /// Name of the network to stop
    #[arg(default_value = DEFAULT_LOCAL_NETWORK_NAME)]
    name: String,
}

pub async fn exec(ctx: &Context, cmd: &Cmd) -> Result<(), anyhow::Error> {
    // Load project
    let p = ctx.project.load().await?;

    // Obtain network configuration
    let network = p
        .networks
        .get(&cmd.name)
        .ok_or_else(|| anyhow!("project does not contain a network named '{}'", cmd.name))?;

    if let Configuration::Connected { connected: _ } = &network.configuration {
        bail!("network '{}' is not a managed network", cmd.name)
    };

    // Network directory
    let nd = ctx.network.get_network_directory(network)?;

    let descriptor = nd
        .load_network_descriptor()
        .await?
        .ok_or_else(|| anyhow!("network '{}' is not running", cmd.name))?;

    match &descriptor.child_locator {
        ChildLocator::Pid(pid) => {
            let _ = ctx
                .term
                .write_line(&format!("Stopping background network (PID: {})...", pid));
        }
        ChildLocator::Container { id, .. } => {
            let _ = ctx.term.write_line(&format!(
                "Stopping background network (container ID: {})...",
                id
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
