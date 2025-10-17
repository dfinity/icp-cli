use std::sync::Arc;

use anyhow::Context;
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

pub(crate) struct Starter {
    agent: Agent,
}

impl Starter {
    pub(crate) fn arc(agent: &Agent) -> Arc<dyn Start> {
        Arc::new(Starter {
            agent: agent.to_owned(),
        })
    }
}

#[async_trait]
impl Start for Starter {
    async fn start(&self, cid: &Principal) -> Result<(), StartError> {
        // Management Interface
        let mgmt = ic_utils::interfaces::ManagementCanister::create(&self.agent);

        // Instruct management canister to start canister
        mgmt.start_canister(cid)
            .await
            .context("failed to start canister")?;

        Ok(())
    }
}
