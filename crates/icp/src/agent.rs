use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use ic_agent::{Agent, AgentError, Identity};

use crate::prelude::*;

#[derive(Debug, thiserror::Error)]
pub enum CreateAgentError {
    #[error(transparent)]
    Agent(#[from] AgentError),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub trait Create: Sync + Send {
    async fn create(&self, id: Arc<dyn Identity>, url: &str) -> Result<Agent, CreateAgentError>;
}

pub struct Creator;

#[async_trait]
impl Create for Creator {
    async fn create(&self, id: Arc<dyn Identity>, url: &str) -> Result<Agent, CreateAgentError> {
        let b = Agent::builder();

        // Url
        let b = b.with_url(url);

        // Identity
        let b = b.with_arc_identity(id);

        // Ingress Expiration
        let b = b.with_ingress_expiry(Duration::from_secs(4 * MINUTE));

        Ok(b.build()?)
    }
}
