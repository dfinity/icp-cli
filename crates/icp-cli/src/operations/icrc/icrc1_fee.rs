use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use candid::{Decode, Encode, Nat, Principal};
use ic_agent::Agent;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Icrc1FeeError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub(crate) trait Icrc1Fee: Sync + Send {
    async fn icrc1_fee(&self) -> Result<Nat, Icrc1FeeError>;
}

pub(crate) struct Icrc1Feeer {
    agent: Agent,
    token_canister: Principal,
}

impl Icrc1Feeer {
    pub(crate) fn arc(agent: &Agent, token_canister: Principal) -> Arc<dyn Icrc1Fee> {
        Arc::new(Icrc1Feeer {
            agent: agent.to_owned(),
            token_canister,
        })
    }
}

#[async_trait]
impl Icrc1Fee for Icrc1Feeer {
    async fn icrc1_fee(&self) -> Result<Nat, Icrc1FeeError> {
        let resp = self
            .agent
            .query(&self.token_canister, "icrc1_fee")
            .with_arg(Encode!(&()).expect("failed to encode arg"))
            .await
            .context("failed to query icrc1 fee")?;

        // Decode response
        Ok::<_, Icrc1FeeError>(Decode!(&resp, Nat).context("failed to decode icrc1 fee")?)
    }
}
