use anyhow::{anyhow, bail};
use clap::{Args, ValueEnum};
use icp::{context::Context, network::Configuration, project::DEFAULT_LOCAL_NETWORK_NAME};

/// Get information about a running network
#[derive(Args, Debug)]
pub(crate) struct InfoArgs {
    /// The type of information to retrieve
    subject: InfoSubject,

    /// Name of the network
    #[arg(default_value = DEFAULT_LOCAL_NETWORK_NAME)]
    name: String,
}

#[derive(Debug, Clone, ValueEnum)]
enum InfoSubject {
    /// Get the port the network gateway is listening on
    Port,
    /// Get the principal of the Candid UI canister
    CandidUiPrincipal,
}

pub(crate) async fn exec(ctx: &Context, args: &InfoArgs) -> Result<(), anyhow::Error> {
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

    // Output based on subject
    match args.subject {
        InfoSubject::Port => {
            ctx.term.write_line(&descriptor.gateway.port.to_string())?;
        }
        InfoSubject::CandidUiPrincipal => match descriptor.candid_ui_canister_id {
            Some(principal) => {
                ctx.term.write_line(&principal.to_string())?;
            }
            None => {
                bail!("Candid UI canister is not installed on this network");
            }
        },
    }

    Ok(())
}
