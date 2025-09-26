use crate::context::{Context, ContextAgentError};
use crate::options::EnvironmentOpt;
use clap::Parser;
use ic_agent::agent::status::Status;
use ic_agent::{Agent, AgentError};
use snafu::Snafu;
use std::time::Duration;

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

pub async fn exec(ctx: &Context, cmd: Cmd) -> Result<(), CommandError> {
    let network = cmd
        .positional_network_name
        .unwrap_or(cmd.network.name().to_string());

    ctx.require_network(&network);
    ctx.require_identity(Some("anonymous"));

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

async fn ping_until_healthy(agent: &Agent) -> Result<Status, TimeoutWaitingForHealthyError> {
    let mut retries = 0;

    loop {
        let status = agent.status().await;
        if let Ok(status) = status {
            let healthy = match &status.replica_health_status {
                Some(s) if s == "healthy" => true,
                None => true,
                _ => false,
            };
            if healthy {
                break Ok(status);
            } else {
                eprintln!("{}", status);
            }
        }
        if retries >= 60 {
            break Err(TimeoutWaitingForHealthyError {});
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        retries += 1;
    }
}

#[derive(Debug, Snafu)]
pub enum CommandError {
    #[snafu(transparent)]
    GetAgent { source: ContextAgentError },

    #[snafu(display("failed to ping the network"))]
    Status { source: AgentError },

    #[snafu(transparent)]
    TimeoutWaitingForHealthy {
        source: TimeoutWaitingForHealthyError,
    },
}

#[derive(Debug, Snafu)]
#[snafu(display("timed out waiting for replica to become healthy"))]
pub struct TimeoutWaitingForHealthyError {}
