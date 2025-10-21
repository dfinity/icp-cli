use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use candid::{Decode, Encode, Principal};
use ic_agent::Agent;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Icrc1SymbolError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub(crate) trait Icrc1Symbol: Sync + Send {
    async fn icrc1_symbol(&self) -> Result<String, Icrc1SymbolError>;
}

pub(crate) struct Icrc1Symboler {
    agent: Agent,
    token_canister: Principal,
}

impl Icrc1Symboler {
    pub(crate) fn arc(agent: &Agent, token_canister: Principal) -> Arc<dyn Icrc1Symbol> {
        Arc::new(Icrc1Symboler {
            agent: agent.to_owned(),
            token_canister,
        })
    }
}

#[async_trait]
impl Icrc1Symbol for Icrc1Symboler {
    async fn icrc1_symbol(&self) -> Result<String, Icrc1SymbolError> {
        let resp = self
            .agent
            .query(&self.token_canister, "icrc1_symbol")
            .with_arg(Encode!(&()).expect("failed to encode arg"))
            .await
            .context("failed to query icrc1 symbol")?;

        // Decode response
        Ok::<_, Icrc1SymbolError>(Decode!(&resp, String).context("failed to decode icrc1 symbol")?)
    }
}
