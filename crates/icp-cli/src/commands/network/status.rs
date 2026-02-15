use anyhow::Context as _;
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
    managed: bool,
    api_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    gateway_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    candid_ui_principal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    proxy_canister_principal: Option<String>,
    root_key: String,
}

pub(crate) async fn exec(ctx: &Context, args: &StatusArgs) -> Result<(), anyhow::Error> {
    // Load project
    let _ = ctx.project.load().await?;

    // Convert args to selection and get network
    let selection: Result<_, _> = args.network_selection.clone().into();
    let network = ctx.get_network_or_environment(&selection?).await?;
    let network_access = ctx.network.access(&network).await.context(format!(
        "unable to access network '{}', is it running?",
        network.name
    ))?;

    let status = match &network.configuration {
        Configuration::Managed { managed: _ } => {
            // Network directory
            let nd = ctx.network.get_network_directory(&network)?;

            // Load network descriptor
            let descriptor = nd
                .load_network_descriptor()
                .await?
                .ok_or_else(|| anyhow::anyhow!("network '{}' is not running", network.name))?;

            // Build status structure
            NetworkStatus {
                managed: true,
                api_url: network_access.api_url.to_string(),
                gateway_url: network_access.http_gateway_url.map(|u| u.to_string()),
                root_key: hex::encode(network_access.root_key),
                candid_ui_principal: descriptor.candid_ui_canister_id.map(|p| p.to_string()),
                proxy_canister_principal: descriptor.proxy_canister_id.map(|p| p.to_string()),
            }
        }
        Configuration::Connected { connected: _ } => NetworkStatus {
            managed: false,
            api_url: network_access.api_url.to_string(),
            gateway_url: network_access.http_gateway_url.map(|u| u.to_string()),
            candid_ui_principal: None,
            proxy_canister_principal: None,
            root_key: hex::encode(network_access.root_key),
        },
    };

    // Display
    let output = if args.json_format {
        serde_json::to_string_pretty(&status).expect("Serializing network status to JSON failed")
    } else {
        let mut output = String::new();
        output.push_str(&format!("Api Url: {}\n", status.api_url));
        if let Some(gateway_url) = status.gateway_url {
            output.push_str(&format!("Gateway Url: {}\n", gateway_url));
        }
        output.push_str(&format!("Root Key: {}\n", status.root_key));
        if let Some(ref principal) = status.candid_ui_principal {
            output.push_str(&format!("Candid UI Principal: {}\n", principal));
        }
        if let Some(ref principal) = status.proxy_canister_principal {
            output.push_str(&format!("Proxy Canister Principal: {}\n", principal));
        }
        output
    };

    for line in output.lines() {
        ctx.term.write_line(line)?;
    }

    Ok(())
}
