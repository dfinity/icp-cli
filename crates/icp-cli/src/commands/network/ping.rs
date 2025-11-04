use std::time::Duration;

use clap::Args;
use ic_agent::{Agent, AgentError, agent::status::Status};
use icp::{
    agent,
    context::GetAgentForUrlError,
    identity::{self, IdentitySelection},
    network::{self},
};
use tokio::time::sleep;

use icp::context::Context;

/// Try to connect to a network, and print out its status.
#[derive(Args, Debug)]
pub(crate) struct PingArgs {
    /// The compute network to connect to. By default, ping the local network.
    #[arg(value_name = "NETWORK", default_value = "local")]
    network: String,

    /// Repeatedly ping until the replica is healthy or 1 minute has passed.
    #[arg(long)]
    wait_healthy: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("network not found")]
    Network,

    #[error("failed to obtain network access")]
    NetworkAccess(#[from] network::AccessError),

    #[error(transparent)]
    Agent(#[from] agent::CreateAgentError),

    #[error(transparent)]
    Status(#[from] AgentError),

    #[error("timed-out waiting for replica to become healthy")]
    Timeout,

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),

    #[error(transparent)]
    GetAgentForUrl(#[from] GetAgentForUrlError),
}

pub(crate) async fn exec(ctx: &Context, args: &PingArgs) -> Result<(), CommandError> {
    // Load Project
    let p = ctx.project.load().await?;

    // Network
    let network = p.networks.get(&args.network).ok_or(CommandError::Network)?;

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

async fn ping_until_healthy(agent: &Agent) -> Result<Status, CommandError> {
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
            break Err(CommandError::Timeout);
        }

        sleep(Duration::from_secs(1)).await;

        retries += 1;
    }
}
