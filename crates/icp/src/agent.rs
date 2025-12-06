use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use ic_agent::{Agent, AgentError, Identity};
use snafu::prelude::*;

use crate::prelude::*;

#[derive(Debug, Snafu)]
pub enum CreateAgentError {
    #[snafu(display("failed to create agent"))]
    Agent { source: AgentError },
}

#[async_trait]
pub trait Create: Sync + Send {
    async fn create(&self, id: Arc<dyn Identity>, url: &str) -> Result<Agent, CreateAgentError>;
}

pub struct Creator;

#[async_trait]
impl Create for Creator {
    async fn create(&self, id: Arc<dyn Identity>, url: &str) -> Result<Agent, CreateAgentError> {
        let b = Agent::builder()
            .with_url(url)
            .with_arc_identity(id)
            .with_ingress_expiry(Duration::from_secs(4 * MINUTE));

        Ok(b.build().context(AgentSnafu)?)
    }
}
