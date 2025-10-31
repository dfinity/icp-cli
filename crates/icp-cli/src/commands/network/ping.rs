use std::time::Duration;

use clap::Args;
use ic_agent::{Agent, AgentError, agent::status::Status};
use icp::{
    agent,
    identity::{self, IdentitySelection},
    network::{self},
};
use tokio::time::sleep;

use crate::commands::{
    Context,
    args::{NetworkOpt, NetworkSelection},
};

/// Try to connect to a network, and print out its status.
#[derive(Args, Debug)]
pub(crate) struct PingArgs {
    #[command(flatten)]
    network: NetworkOpt,

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
    Agent(#[from] agent::CreateError),

    #[error(transparent)]
    Status(#[from] AgentError),

    #[error("timed-out waiting for replica to become healthy")]
    Timeout,

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

pub(crate) async fn exec(ctx: &Context, args: &PingArgs) -> Result<(), CommandError> {
    // Load Project
    let p = ctx.project.load().await?;

    // Identity
    let id = ctx.identity.load(IdentitySelection::Anonymous).await?;

    // Network
    let network_selection: NetworkSelection = args.network.clone().into();
    let network = match network_selection {
        NetworkSelection::Name(name) | NetworkSelection::Default(name) => {
            p.networks.get(&name).ok_or(CommandError::Network)?
        }
        NetworkSelection::Url(_) => unimplemented!("pinging by url is not supported"),
    };

    // NetworkAccess
    let access = ctx.network.access(network).await?;

    // Agent
    let agent = ctx.agent.create(id, &access.url).await?;

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
