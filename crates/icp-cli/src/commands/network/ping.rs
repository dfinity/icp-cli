use anyhow::{anyhow, bail};
use clap::Args;
use ic_agent::{Agent, agent::status::Status};
use icp::{identity::IdentitySelection, project::DEFAULT_LOCAL_NETWORK_NAME};
use std::time::Duration;
use tokio::time::sleep;

use icp::context::Context;

/// Try to connect to a network, and print out its status.
#[derive(Args, Debug)]
pub(crate) struct PingArgs {
    /// The compute network to connect to. By default, ping the local network.
    #[arg(value_name = "NETWORK", default_value = DEFAULT_LOCAL_NETWORK_NAME)]
    network: String,

    /// Repeatedly ping until the replica is healthy or 1 minute has passed.
    #[arg(long)]
    wait_healthy: bool,
}

pub(crate) async fn exec(ctx: &Context, args: &PingArgs) -> Result<(), anyhow::Error> {
    // Load Project
    let p = ctx.project.load().await?;

    // Obtain network configuration
    let network = p.networks.get(&args.network).ok_or_else(|| {
        anyhow!(
            "project does not contain a network named '{}'",
            args.network
        )
    })?;

    // NetworkAccess
    let access = ctx.network.access(network).await?;

    // Agent
    let agent = ctx
        .get_agent_for_url(&IdentitySelection::Anonymous, &access.url)
        .await?;

    if let Some(k) = access.root_key {
        agent.set_root_key(k);
    }

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
