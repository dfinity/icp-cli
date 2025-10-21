use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use candid::{Decode, Encode, Principal};
use ic_agent::Agent;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Icrc1DecimalsError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub(crate) trait Icrc1Decimals: Sync + Send {
    async fn icrc1_decimals(&self) -> Result<u8, Icrc1DecimalsError>;
}

pub(crate) struct Icrc1Decimalser {
    agent: Agent,
    token_canister: Principal,
}

impl Icrc1Decimalser {
    pub(crate) fn arc(agent: &Agent, token_canister: Principal) -> Arc<dyn Icrc1Decimals> {
        Arc::new(Icrc1Decimalser {
            agent: agent.to_owned(),
            token_canister,
        })
    }
}

#[async_trait]
impl Icrc1Decimals for Icrc1Decimalser {
    async fn icrc1_decimals(&self) -> Result<u8, Icrc1DecimalsError> {
        let resp = self
            .agent
            .query(&self.token_canister, "icrc1_decimals")
            .with_arg(Encode!(&()).expect("failed to encode arg"))
            .await
            .context("failed to query icrc1 decimals")?;

        // Decode response
        Ok::<_, Icrc1DecimalsError>(Decode!(&resp, u8).context("failed to decode icrc1 decimals")?)
    }
}
