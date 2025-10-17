use std::sync::Arc;

use async_trait::async_trait;
use candid::Principal;
use ic_agent::Agent;

#[derive(Debug, thiserror::Error)]
pub(crate) enum StartError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub(crate) trait Start: Sync + Send {
    async fn start(&self, cid: &Principal) -> Result<(), StartError>;
}

pub(crate) struct Starter;

impl Starter {
    pub(crate) fn arc(agent: &Agent) -> Arc<dyn Start> {
        Arc::new(Starter)
    }
}

#[async_trait]
impl Start for Starter {
    async fn start(&self, cid: &Principal) -> Result<(), StartError> {
        Ok(())
    }
}
