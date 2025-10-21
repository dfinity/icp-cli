use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use candid::{Decode, Encode, Nat, Principal};
use ic_agent::Agent;
use icrc_ledger_types::icrc1::account::{Account, Subaccount};

#[derive(Debug, thiserror::Error)]
pub(crate) enum Icrc1BalanceError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub(crate) trait Icrc1Balance: Sync + Send {
    async fn icrc1_balance(
        &self,
        owner: Principal,
        subaccount: Option<Subaccount>,
    ) -> Result<Nat, Icrc1BalanceError>;
}

pub(crate) struct Icrc1Balancer {
    agent: Agent,
    token_canister: Principal,
}

impl Icrc1Balancer {
    pub(crate) fn arc(agent: &Agent, token_canister: Principal) -> Arc<dyn Icrc1Balance> {
        Arc::new(Icrc1Balancer {
            agent: agent.to_owned(),
            token_canister,
        })
    }
}

#[async_trait]
impl Icrc1Balance for Icrc1Balancer {
    async fn icrc1_balance(
        &self,
        owner: Principal,
        subaccount: Option<Subaccount>,
    ) -> Result<Nat, Icrc1BalanceError> {
        let resp = self
            .agent
            .query(&self.token_canister, "icrc1_balance_of")
            .with_arg(Encode!(&Account { owner, subaccount }).expect("failed to encode arg"))
            .await
            .context("failed to query icrc1 balance of")?;

        // Decode response
        Ok::<_, Icrc1BalanceError>(Decode!(&resp, Nat).context("failed to decode icrc1 balance")?)
    }
}
