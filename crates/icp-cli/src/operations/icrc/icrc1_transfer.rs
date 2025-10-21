use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use candid::{Decode, Encode, Nat, Principal};
use ic_agent::Agent;
use icrc_ledger_types::icrc1::{
    account::Account,
    transfer::{TransferArg, TransferError},
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum Icrc1TransferError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[async_trait]
pub(crate) trait Icrc1Transfer: Sync + Send {
    async fn icrc1_transfer(
        &self,
        amount: Nat,
        to: Principal,
    ) -> Result<Result<Nat, TransferError>, Icrc1TransferError>;
}

pub(crate) struct Icrc1Transferrer {
    agent: Agent,
    token_canister: Principal,
}

impl Icrc1Transferrer {
    pub(crate) fn arc(agent: &Agent, token_canister: Principal) -> Arc<dyn Icrc1Transfer> {
        Arc::new(Icrc1Transferrer {
            agent: agent.to_owned(),
            token_canister,
        })
    }
}

#[async_trait]
impl Icrc1Transfer for Icrc1Transferrer {
    async fn icrc1_transfer(
        &self,
        amount: Nat,
        to: Principal,
    ) -> Result<Result<Nat, TransferError>, Icrc1TransferError> {
        let resp = self
            .agent
            .update(&self.token_canister, "icrc1_transfer")
            .with_arg(
                Encode!(&TransferArg {
                    amount,
                    to: Account {
                        owner: to,
                        subaccount: None
                    },
                    from_subaccount: None,
                    fee: None,
                    created_at_time: None,
                    memo: None,
                })
                .expect("failed to encode arg"),
            )
            .await
            .context("failed to query icrc1 balance of")?;

        // Decode response
        Ok::<_, Icrc1TransferError>(
            Decode!(&resp, Result<Nat, TransferError>).context("failed to decode icrc1 balance")?,
        )
    }
}
