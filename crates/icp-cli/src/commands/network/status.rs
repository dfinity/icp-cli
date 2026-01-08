use anyhow::{anyhow, bail};
use clap::{Args, ValueEnum};
use icp::{context::Context, network::Configuration, project::DEFAULT_LOCAL_NETWORK_NAME};
use serde::Serialize;

/// Get status information about a running network
#[derive(Args, Debug)]
pub(crate) struct StatusArgs {
    /// The specific field to retrieve (if not provided, shows all fields)
    subject: Option<StatusSubject>,

    /// Name of the network
    #[arg(default_value = DEFAULT_LOCAL_NETWORK_NAME)]
    name: String,

    /// Format output as JSON
    #[arg(long = "json")]
    json_format: bool,
}

#[derive(Debug, Clone, ValueEnum)]
enum StatusSubject {
    /// Get the port the network gateway is listening on
    Port,
    /// Get the principal of the Candid UI canister
    CandidUiPrincipal,
    /// Get the network's root key (public key)
    RootKey,
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
    let p = ctx.project.load().await?;

    // Obtain network configuration
    let network = p
        .networks
        .get(&args.name)
        .ok_or_else(|| anyhow!("project does not contain a network named '{}'", args.name))?;

    // Ensure it's a managed network
    if let Configuration::Connected { connected: _ } = &network.configuration {
        bail!("network '{}' is not a managed network", args.name)
    };

    // Network directory
    let nd = ctx.network.get_network_directory(network)?;

    // Load network descriptor
    let descriptor = nd
        .load_network_descriptor()
        .await?
        .ok_or_else(|| anyhow!("network '{}' is not running", args.name))?;

    // Build status structure
    let status = NetworkStatus {
        port: descriptor.gateway.port,
        root_key: hex::encode(&descriptor.root_key),
        candid_ui_principal: descriptor.candid_ui_canister_id.map(|p| p.to_string()),
    };

    // Output based on format and subject
    let output = if args.json_format {
        serde_json::to_string_pretty(&status).expect("Serializing network status to JSON failed")
    } else if let Some(ref subject) = args.subject {
        match subject {
            StatusSubject::Port => status.port.to_string(),
            StatusSubject::CandidUiPrincipal => status
                .candid_ui_principal
                .ok_or_else(|| anyhow!("Candid UI canister is not installed on this network"))?,
            StatusSubject::RootKey => status.root_key,
        }
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
