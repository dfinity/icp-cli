use std::time::Duration;

use anyhow::Context as _;
use clap::Parser;
use ic_agent::{Agent, AgentError, agent::status::Status};
use icp::{
    agent,
    identity::{self, IdentitySelection},
    network::access::get_network_access,
};
use tokio::time::sleep;

use crate::commands::Context;

/// Try to connect to a network, and print out its status.
#[derive(Parser, Debug)]
pub struct Cmd {
    /// The compute network to connect to. By default, ping the local network.
    #[arg(value_name = "NETWORK", default_value = "local")]
    network: String,

    /// Repeatedly ping until the replica is healthy or 1 minute has passed.
    #[arg(long)]
    wait_healthy: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Project(#[from] icp::LoadError),

    #[error("network not found")]
    Network,

    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error(transparent)]
    Agent(#[from] agent::CreateError),

    #[error(transparent)]
    Status(#[from] AgentError),

    #[error("timed-out waiting for replica to become healthy")]
    Timeout,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    // Load Project
    let p = ctx.project.load().await?;

    // Identity
    let id = ctx.identity.load(IdentitySelection::Anonymous).await?;

    // Network
    let network = p.networks.get(&cmd.network).ok_or(CommandError::Network)?;

    // NetworkAccess
    let acceess = get_network_access(nd, network).context("failed to load network access")?;

    // Agent
    let mut agent = ctx.agent.create(id, &acceess.url).await?;

    if let Some(k) = acceess.root_key {
        agent.set_root_key(k);
    }

    // Query
    let status = match cmd.wait_healthy {
        // wait
        true => ping_until_healthy(&agent).await?,

        // dont wait
        false => agent.status().await?,
    };

    println!("{}", status);

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

            eprintln!("{}", status);
        }

        if retries >= 60 {
            break Err(CommandError::Timeout);
        }

        sleep(Duration::from_secs(1)).await;

        retries += 1;
    }
}
