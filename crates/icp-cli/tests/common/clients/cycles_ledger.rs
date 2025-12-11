use candid::{Decode, Encode, Nat, Principal};
use ic_agent::Agent;
use icp_canister_interfaces::cycles_ledger::CYCLES_LEDGER_PRINCIPAL;
use icrc_ledger_types::icrc1::account::{Account, Subaccount};

use crate::common::TestContext;

pub(crate) struct Client {
    agent: Agent,
}

impl Client {
    pub(crate) fn new(ctx: &TestContext) -> Self {
        Self { agent: ctx.agent() }
    }

    pub(crate) async fn balance_of(&self, owner: Principal, subaccount: Option<Subaccount>) -> Nat {
        let args = Account { owner, subaccount };
        let bytes = Encode!(&args).unwrap();
        let result = &self
            .agent
            .query(&CYCLES_LEDGER_PRINCIPAL, "icrc1_balance_of")
            .with_arg(bytes)
            .await
            .unwrap();
        Decode!(result, Nat).unwrap()
    }
}
