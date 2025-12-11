use candid::{Decode, Encode, Nat, Principal};
use ic_agent::Agent;
use icp_canister_interfaces::icp_ledger::ICP_LEDGER_PRINCIPAL;
use icrc_ledger_types::icrc1::{
    account::{Account, Subaccount},
    transfer::TransferArg,
};

use crate::common::TestContext;

pub(crate) struct Client {
    agent: Agent,
}

impl Client {
    pub(crate) fn new(ctx: &TestContext) -> Self {
        Self { agent: ctx.agent() }
    }

    pub(crate) async fn balance_of(&self, owner: Principal, subaccount: Option<Subaccount>) -> Nat {
        let arg = Account { owner, subaccount };
        let bytes = Encode!(&arg).unwrap();
        let result = &self
            .agent
            .query(&ICP_LEDGER_PRINCIPAL, "icrc1_balance_of")
            .with_arg(bytes)
            .await
            .unwrap();
        Decode!(result, Nat).unwrap()
    }

    pub(crate) async fn acquire_icp(
        &self,
        owner: Principal,
        subaccount: Option<Subaccount>,
        amount: impl Into<Nat>,
    ) {
        let arg = TransferArg {
            from_subaccount: None,
            to: icrc_ledger_types::icrc1::account::Account { owner, subaccount },
            fee: None,
            created_at_time: None,
            memo: None,
            amount: amount.into(),
        };
        let bytes = Encode!(&arg).unwrap();
        self.agent
            .update(&ICP_LEDGER_PRINCIPAL, "icrc1_transfer")
            .with_arg(bytes)
            .await
            .unwrap();
    }
}
