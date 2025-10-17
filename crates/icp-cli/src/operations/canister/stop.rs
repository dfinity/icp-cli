use std::sync::Arc;

use async_trait::async_trait;
use candid::Principal;
use ic_agent::Agent;

#[derive(Debug, thiserror::Error)]
pub(crate) enum StopError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub(crate) trait Stop: Sync + Send {
    async fn stop(&self, cid: &Principal) -> Result<(), StopError>;
}

pub(crate) struct Stopper;

impl Stopper {
    pub(crate) fn arc(agent: &Agent) -> Arc<dyn Stop> {
        Arc::new(Stopper)
    }
}

#[async_trait]
impl Stop for Stopper {
    async fn stop(&self, cid: &Principal) -> Result<(), StopError> {
        Ok(())
    }
}
