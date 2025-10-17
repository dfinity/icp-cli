use std::sync::Arc;

use anyhow::Context;
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

pub(crate) struct Stopper {
    agent: Agent,
}

impl Stopper {
    pub(crate) fn arc(agent: &Agent) -> Arc<dyn Stop> {
        Arc::new(Stopper {
            agent: agent.to_owned(),
        })
    }
}

#[async_trait]
impl Stop for Stopper {
    async fn stop(&self, cid: &Principal) -> Result<(), StopError> {
        // Management Interface
        let mgmt = ic_utils::interfaces::ManagementCanister::create(&self.agent);

        // Instruct management canister to stop canister
        mgmt.stop_canister(cid)
            .await
            .context("failed to stop canister")?;

        Ok(())
    }
}
