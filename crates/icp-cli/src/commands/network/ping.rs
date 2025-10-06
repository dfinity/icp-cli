use std::time::Duration;

use clap::Parser;
use ic_agent::agent::status::Status;
use ic_agent::{Agent, AgentError};
use icp::identity::{self, IdentitySelection};
use tokio::time::sleep;

use crate::{context::Context, options::EnvironmentOpt};

/// Try to connect to a network, and print out its status.
#[derive(Parser, Debug)]
pub struct Cmd {
    #[command(flatten)]
    network: EnvironmentOpt,

    /// The compute network to connect to. By default, ping the local network.
    #[arg(group = "network-select", value_name = "NETWORK")]
    positional_network_name: Option<String>,

    /// Repeatedly ping until the replica is healthy or 1 minute has passed.
    #[arg(long)]
    wait_healthy: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Identity(#[from] identity::LoadError),

    #[error("failed to create agent")]
    Agent { err: AgentError },

    #[error("failed to query network status")]
    Status { err: AgentError },

    #[error("timed-out waiting for replica to become healthy")]
    Timeout,
}

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    let network = cmd
        .positional_network_name
        .unwrap_or(cmd.network.name().to_string());

    // ctx.require_network(&network);

    let id = ctx.identity.load(IdentitySelection::Anonymous).await?;

    let agent = ctx.agent()?;

    let status = if cmd.wait_healthy {
        ping_until_healthy(agent).await?
    } else {
        agent
            .status()
            .await
            .map_err(|source| CommandError::Status { source })?
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
