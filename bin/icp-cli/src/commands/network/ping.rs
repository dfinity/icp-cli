use crate::env::{Env, EnvGetAgentError};
use crate::options::NetworkOpt;
use clap::Parser;
use ic_agent::agent::status::Status;
use ic_agent::{Agent, AgentError};
use snafu::Snafu;
use std::time::Duration;

/// Try to connect to a network, and print out its status.
#[derive(Parser, Debug)]
pub struct PingCmd {
    #[clap(flatten)]
    network: NetworkOpt,

    /// The compute network to connect to. By default, ping the local network.
    #[clap(group = "network-select", value_name = "NETWORK")]
    positional_network_name: Option<String>,

    /// Repeatedly ping until the replica is healthy or 1 minute has passed.
    #[clap(long)]
    wait_healthy: bool,
}

pub async fn exec(env: &Env, cmd: PingCmd) -> Result<(), PingNetworkCommandError> {
    env.require_identity(Some("anonymous"));
    let network = cmd
        .positional_network_name
        .unwrap_or(cmd.network.name().to_string());
    env.require_network(&network);

    let agent = env.agent()?;

    let status = if cmd.wait_healthy {
        ping_until_healthy(agent).await?
    } else {
        agent
            .status()
            .await
            .map_err(|source| PingNetworkCommandError::Status { source })?
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
pub enum PingNetworkCommandError {
    #[snafu(transparent)]
    GetAgent { source: EnvGetAgentError },

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
