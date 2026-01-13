use anyhow::bail;
use clap::Args;
use icp::{context::Context, network::Configuration};
use serde::Serialize;

use super::args::NetworkOrEnvironmentArgs;

/// Get status information about a running network
#[derive(Args, Debug)]
#[command(after_long_help = "\
Examples:

    # Get status of default 'local' network
    icp network status
  
    # Get status of explicit network
    icp network status mynetwork
  
    # Get status using environment flag
    icp network status -e staging
  
    # Get status using ICP_ENVIRONMENT variable
    ICP_ENVIRONMENT=staging icp network status
  
    # Name overrides ICP_ENVIRONMENT
    ICP_ENVIRONMENT=staging icp network status local
  
    # JSON output
    icp network status --json
")]
pub(crate) struct StatusArgs {
    #[clap(flatten)]
    network_selection: NetworkOrEnvironmentArgs,

    /// Format output as JSON
    #[arg(long = "json")]
    json_format: bool,
}

#[derive(Debug, Serialize)]
struct NetworkStatus {
    port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    candid_ui_principal: Option<String>,
    root_key: String,
}

pub(crate) async fn exec(ctx: &Context, args: &StatusArgs) -> Result<(), anyhow::Error> {
    // Load project
    let _ = ctx.project.load().await?;

    // Convert args to selection and get network
    let selection: Result<_, _> = args.network_selection.clone().into();
    let network = ctx.get_network_or_environment(&selection?).await?;

    // Ensure it's a managed network
    if let Configuration::Connected { connected: _ } = &network.configuration {
        bail!("network '{}' is not a managed network", network.name)
    };

    // Network directory
    let nd = ctx.network.get_network_directory(&network)?;

    // Load network descriptor
    let descriptor = nd
        .load_network_descriptor()
        .await?
        .ok_or_else(|| anyhow::anyhow!("network '{}' is not running", network.name))?;

    // Build status structure
    let status = NetworkStatus {
        port: descriptor.gateway.port,
        root_key: hex::encode(&descriptor.root_key),
        candid_ui_principal: descriptor.candid_ui_canister_id.map(|p| p.to_string()),
    };

    // Display
    let output = if args.json_format {
        serde_json::to_string_pretty(&status).expect("Serializing network status to JSON failed")
    } else {
        let mut output = String::new();
        output.push_str(&format!("Port: {}\n", status.port));
        output.push_str(&format!("Root Key: {}\n", status.root_key));
        if let Some(ref principal) = status.candid_ui_principal {
            output.push_str(&format!("Candid UI Principal: {}\n", principal));
        }
        output
    };

    for line in output.lines() {
        ctx.term.write_line(line)?;
    }

    Ok(())
}
