use anyhow::bail;
use clap::Args;
use ic_agent::{Agent, agent::status::Status};
use icp::identity::IdentitySelection;
use std::time::Duration;
use tokio::time::sleep;

use super::args::NetworkOrEnvironmentArgs;
use icp::context::Context;

/// Try to connect to a network, and print out its status.
#[derive(Args, Debug)]
#[command(after_long_help = "\
Examples:

    # Ping default 'local' network
    icp network ping
  
    # Ping explicit network
    icp network ping mynetwork
  
    # Ping using environment flag
    icp network ping -e staging
  
    # Ping using ICP_ENVIRONMENT variable
    ICP_ENVIRONMENT=staging icp network ping
  
    # Name overrides ICP_ENVIRONMENT
    ICP_ENVIRONMENT=staging icp network ping local
  
    # Wait until healthy
    icp network ping --wait-healthy
")]
pub(crate) struct PingArgs {
    #[clap(flatten)]
    network_selection: NetworkOrEnvironmentArgs,

    /// Repeatedly ping until the replica is healthy or 1 minute has passed.
    #[arg(long)]
    wait_healthy: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &PingArgs) -> Result<(), anyhow::Error> {
    // Load project
    let _ = ctx.project.load().await?;

    // Convert args to selection and get network
    let selection: Result<_, _> = args.network_selection.clone().into();
    let network = ctx.get_network_or_environment(&selection?).await?;

    // NetworkAccess
    let access = ctx.network.access(&network).await?;

    // Agent
    // TODO We might want to expose the ctx.create_agent function that takes a NetworkAccess
    // instead of doing this
    let agent = ctx
        .get_agent_for_url(&IdentitySelection::Anonymous, &access.api_url)
        .await?;

    agent.set_root_key(access.root_key);

    // Query
    let status = match args.wait_healthy {
        // wait
        true => ping_until_healthy(&agent).await?,

        // dont wait
        false => agent.status().await?,
    };

    println!("{status}");

    Ok(())
}

async fn ping_until_healthy(agent: &Agent) -> Result<Status, anyhow::Error> {
    let mut retries = 0;

    loop {
        if let Ok(status) = agent.status().await {
            let is_ok = match &status.replica_health_status {
                // Ok
                Some(s) if s == "healthy" => true,

                // Ok
                None => true,

                // Fail
                _ => false,
            };

            if is_ok {
                return Ok(status);
            }

            eprintln!("{status}");
        }

        if retries >= 60 {
            bail!("timed-out waiting for replica to become healthy");
        }

        sleep(Duration::from_secs(1)).await;

        retries += 1;
    }
}
