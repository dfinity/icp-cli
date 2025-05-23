use ic_agent::{Agent, AgentError};
use reqwest::Url;
use snafu::prelude::*;
use std::time::Duration;

#[derive(Debug, Snafu)]
pub enum PingAndWaitError {
    #[snafu(display("failed to build agent for url {}", url))]
    BuildAgent {
        source: AgentError,
        url: String,
    },

    Timeout {
        source: AgentError,
    },
}

pub async fn ping_and_wait(url: &str) -> Result<(), PingAndWaitError> {
    let agent = Agent::builder()
        .with_url(url)
        .build()
        .context(BuildAgentSnafu { url })?;
    let mut retries = 0;
    loop {
        let status = agent.status().await;
        match status {
            Ok(status) => {
                if matches!(&status.replica_health_status, Some(status) if status == "healthy") {
                    break;
                }
            }
            Err(e) => {
                if retries >= 60 {
                    return Err(PingAndWaitError::Timeout { source: e });
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
                retries += 1;
            }
        }
    }
    Ok(())
}
